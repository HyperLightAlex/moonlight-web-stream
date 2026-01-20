# Backlight Integration - Implementation Guide

**Version:** 1.0  
**Date:** January 2026  
**Parent Document:** [BACKLIGHT_INTEGRATION.md](./BACKLIGHT_INTEGRATION.md)

---

## Table of Contents

1. [Implementation Checklist](#1-implementation-checklist)
2. [Backlight Desktop Code Examples](#2-backlight-desktop-code-examples)
3. [Web Server Modifications](#3-web-server-modifications)
4. [Android Client Updates](#4-android-client-updates)
5. [Testing Strategy](#5-testing-strategy)
6. [Troubleshooting Guide](#6-troubleshooting-guide)

---

## 1. Implementation Checklist

### 1.1 Phase 1: Core Embedding

#### Backend Team (moonlight-web-stream)

- [ ] Verify Windows x86_64 build works with `cross build --release --target x86_64-pc-windows-gnu`
- [ ] Create Windows MSVC build configuration (optional, for better compatibility)
- [ ] Test web-server.exe runs standalone on Windows
- [ ] Test streamer.exe runs when spawned by web-server
- [ ] Document any Windows-specific dependencies (VC++ Runtime, etc.)

#### Desktop Team (Backlight)

- [ ] Add `resources/moonlight-web/` directory to project
- [ ] Add web-server.exe and streamer.exe to build resources
- [ ] Create `WebServerManager` class in `src/main/lib/moonlight-web/`
- [ ] Implement process spawn with proper working directory
- [ ] Implement graceful shutdown on app quit
- [ ] Implement crash recovery with restart logic
- [ ] Add dynamic port selection using existing `findAvailablePortPair()`
- [ ] Add Windows Firewall rule for web-server.exe
- [ ] Create config.json template in resources
- [ ] Copy config to userData on first launch

### 1.2 Phase 2: Auto-Configuration

#### Backend Team

- [ ] Implement `--bootstrap` CLI flag
- [ ] Implement `--bootstrap-user` and `--bootstrap-password` flags
- [ ] Implement `--bootstrap-host` and `--bootstrap-host-port` flags
- [ ] Auto-create user if not exists in bootstrap mode
- [ ] Auto-add localhost host in bootstrap mode
- [ ] Auto-pair with host (using existing Fuji OTP logic)
- [ ] Output bootstrap result as JSON to stdout
- [ ] Implement `/api/health` endpoint

#### Desktop Team

- [ ] Parse bootstrap JSON output from web-server stdout
- [ ] Store credentials in settings (encrypted with safeStorage)
- [ ] Store server port and connection info
- [ ] Implement health check polling
- [ ] Display web server status in UI (optional)

### 1.3 Phase 3: Dual QR Code UI

#### Desktop Team

- [ ] Add "Connect Android" button/tab to pairing modal
- [ ] Implement Android QR code generation with stored credentials
- [ ] Include remote access info in QR code (if available)
- [ ] Keep existing iOS QR code flow unchanged
- [ ] Add visual indicator for remote access availability
- [ ] Display user-friendly messages for NAT issues

### 1.4 Phase 4: Android Client Updates

#### Android Team

- [ ] Add QR code schema detection (backlight-webserver vs fuji-pairing)
- [ ] Implement credential parsing from QR code
- [ ] Implement secure credential storage (Android Keystore)
- [ ] Implement login API call on first connection
- [ ] Implement session token storage
- [ ] Implement automatic re-authentication on 401
- [ ] Update host list to use web server API
- [ ] Update streaming to use web server WebSocket
- [ ] Handle remote URL vs local URL based on network

### 1.5 Phase 5: Testing & Polish

- [ ] Integration test: Local streaming (same network)
- [ ] Integration test: Remote streaming (different network, UPnP)
- [ ] Integration test: Remote streaming (manual port forward)
- [ ] Test: Session expiry and re-authentication
- [ ] Test: Web server crash recovery
- [ ] Test: Backlight restart preserves credentials
- [ ] Test: Multiple app launches without restart
- [ ] Performance test: Latency comparison vs direct Moonlight
- [ ] Edge case: Firewall blocking
- [ ] Edge case: Symmetric NAT detection and user guidance

---

## 2. Backlight Desktop Code Examples

### 2.1 WebServerManager Class

```typescript
// src/main/lib/moonlight-web/WebServerManager.ts

import { spawn, ChildProcess, exec } from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import { app } from 'electron';
import { EventEmitter } from 'events';
import { findAvailablePort } from '../windows/port-utils';
import { addFirewallRule } from '../windows/firewall';
import { log, logError, logWarn } from '../logger';

interface BootstrapResult {
  success: boolean;
  user?: {
    id: number;
    username: string;
    password: string;
  };
  host?: {
    id: number;
    name: string;
    paired: boolean;
  };
  server?: {
    port: number;
    external_ip: string | null;
    upnp_success: boolean;
    nat_type: string;
  };
  error?: string;
}

interface WebServerConfig {
  port: number;
  configPath: string;
  dataPath: string;
  executablePath: string;
  streamerPath: string;
}

interface WebServerHealth {
  status: string;
  uptime_secs: number;
  active_streams: number;
  hosts: {
    total: number;
    paired: number;
    online: number;
  };
  remote_access: {
    upnp_enabled: boolean;
    upnp_success: boolean;
    external_ip: string | null;
    nat_type: string;
  };
}

export class WebServerManager extends EventEmitter {
  private process: ChildProcess | null = null;
  private config: WebServerConfig | null = null;
  private bootstrapResult: BootstrapResult | null = null;
  private restartAttempts = 0;
  private maxRestartAttempts = 5;
  private restartDelayMs = 2000;
  private healthCheckInterval: NodeJS.Timer | null = null;
  private isShuttingDown = false;

  private readonly bundleDir: string;
  private readonly dataDir: string;

  constructor() {
    super();
    
    if (app.isPackaged) {
      this.bundleDir = path.join(process.resourcesPath, 'moonlight-web');
    } else {
      this.bundleDir = path.join(app.getAppPath(), 'resources', 'moonlight-web');
    }
    
    this.dataDir = path.join(app.getPath('userData'), 'moonlight-web');
  }

  /**
   * Initialize the web server - call this on app startup
   */
  async initialize(): Promise<BootstrapResult> {
    log('web-server', 'Initializing web server...');
    
    // Ensure directories exist
    await this.ensureDirectories();
    
    // Find available port
    const port = await this.findPort();
    log('web-server', `Selected port: ${port}`);
    
    // Setup configuration
    this.config = {
      port,
      configPath: path.join(this.dataDir, 'config.json'),
      dataPath: path.join(this.dataDir, 'data.json'),
      executablePath: path.join(this.bundleDir, 'web-server.exe'),
      streamerPath: path.join(this.bundleDir, 'streamer.exe'),
    };
    
    // Create/update config file
    await this.writeConfig();
    
    // Add firewall rule
    await this.setupFirewall();
    
    // Start the server
    const result = await this.start();
    
    // Start health monitoring
    this.startHealthCheck();
    
    return result;
  }

  private async ensureDirectories(): Promise<void> {
    const dirs = [
      this.dataDir,
      path.join(this.dataDir, 'logs'),
    ];
    
    for (const dir of dirs) {
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
    }
  }

  private async findPort(): Promise<number> {
    // Try default port first, then find available
    const preferredPorts = [8080, 8081, 8082, 8083, 8084];
    
    for (const port of preferredPorts) {
      const available = await this.isPortAvailable(port);
      if (available) {
        return port;
      }
    }
    
    // Fall back to any available port
    return findAvailablePort(8080, 8100);
  }

  private async isPortAvailable(port: number): Promise<boolean> {
    return new Promise((resolve) => {
      const net = require('net');
      const server = net.createServer();
      
      server.listen(port, () => {
        server.close(() => resolve(true));
      });
      
      server.on('error', () => resolve(false));
    });
  }

  private async writeConfig(): Promise<void> {
    if (!this.config) return;
    
    const config = {
      data_storage: {
        type: 'json',
        path: this.config.dataPath,
        session_expiration_check_interval: { secs: 300, nanos: 0 }
      },
      web_server: {
        bind_address: `0.0.0.0:${this.config.port}`,
        first_login_create_admin: false,
        session_cookie_expiration: { secs: 86400, nanos: 0 }
      },
      moonlight: {
        default_http_port: 48989,
        pair_device_name: 'Backlight-WebServer'
      },
      webrtc: {
        ice_servers: [
          {
            is_default: true,
            urls: [
              'stun:stun.l.google.com:19302',
              'stun:stun1.l.google.com:3478',
              'stun:stun.cloudflare.com:3478'
            ]
          }
        ],
        port_range: { min: 40000, max: 40100 },
        network_types: ['udp4', 'udp6']
      },
      upnp: {
        enabled: true,
        lease_duration_secs: 3600,
        description: 'Backlight Web Streaming'
      },
      remote: {
        enabled: true,
        stun_discovery: true
      },
      turn: {
        enabled: false,
        urls: [],
        username: '',
        credential: ''
      },
      streamer_path: this.config.streamerPath,
      log: {
        level_filter: 'Info',
        file_path: path.join(this.dataDir, 'logs', 'web-server.log')
      }
    };
    
    fs.writeFileSync(this.config.configPath, JSON.stringify(config, null, 2));
    log('web-server', `Config written to ${this.config.configPath}`);
  }

  private async setupFirewall(): Promise<void> {
    if (!this.config) return;
    
    try {
      await addFirewallRule('Backlight Web Server', this.config.port, 'TCP');
      log('web-server', 'Firewall rule added');
    } catch (error) {
      logWarn('web-server', 'Failed to add firewall rule:', error);
    }
  }

  /**
   * Start the web server process
   */
  async start(): Promise<BootstrapResult> {
    if (!this.config) {
      throw new Error('WebServerManager not initialized');
    }

    const args = [
      '--config', this.config.configPath,
      '--bind', `0.0.0.0:${this.config.port}`,
      '--bootstrap',
      '--bootstrap-user', 'backlight',
    ];

    log('web-server', `Starting: ${this.config.executablePath} ${args.join(' ')}`);

    return new Promise((resolve, reject) => {
      this.process = spawn(this.config!.executablePath, args, {
        cwd: this.bundleDir,
        stdio: ['ignore', 'pipe', 'pipe'],
        windowsHide: true,
      });

      let stdoutBuffer = '';
      let stderrBuffer = '';
      let bootstrapReceived = false;

      // Capture stdout for bootstrap result
      this.process.stdout?.on('data', (data) => {
        const text = data.toString();
        stdoutBuffer += text;
        
        // Look for bootstrap JSON
        if (!bootstrapReceived) {
          const lines = stdoutBuffer.split('\n');
          for (const line of lines) {
            try {
              const parsed = JSON.parse(line.trim());
              if (parsed.success !== undefined) {
                bootstrapReceived = true;
                this.bootstrapResult = parsed;
                log('web-server', 'Bootstrap result:', parsed);
                resolve(parsed);
              }
            } catch {
              // Not JSON, continue buffering
            }
          }
        }
        
        // Log non-JSON output
        log('web-server', `[stdout] ${text.trim()}`);
      });

      this.process.stderr?.on('data', (data) => {
        stderrBuffer += data.toString();
        log('web-server', `[stderr] ${data.toString().trim()}`);
      });

      this.process.on('error', (error) => {
        logError('web-server', 'Process error:', error);
        if (!bootstrapReceived) {
          reject(error);
        }
      });

      this.process.on('exit', (code, signal) => {
        log('web-server', `Process exited: code=${code}, signal=${signal}`);
        
        if (!bootstrapReceived) {
          reject(new Error(`Process exited before bootstrap: ${stderrBuffer}`));
        }
        
        this.process = null;
        
        // Auto-restart if not shutting down
        if (!this.isShuttingDown && this.restartAttempts < this.maxRestartAttempts) {
          this.restartAttempts++;
          logWarn('web-server', `Restarting (attempt ${this.restartAttempts})...`);
          setTimeout(() => this.start(), this.restartDelayMs);
        }
        
        this.emit('exit', code, signal);
      });

      this.process.on('spawn', () => {
        log('web-server', 'Process spawned successfully');
        
        // Reset restart counter after successful run
        setTimeout(() => {
          this.restartAttempts = 0;
        }, 30000);
      });

      // Timeout for bootstrap
      setTimeout(() => {
        if (!bootstrapReceived) {
          reject(new Error('Bootstrap timeout - no response from server'));
        }
      }, 30000);
    });
  }

  /**
   * Stop the web server process
   */
  async stop(): Promise<void> {
    this.isShuttingDown = true;
    this.stopHealthCheck();
    
    if (!this.process) {
      return;
    }

    log('web-server', 'Stopping web server...');

    return new Promise((resolve) => {
      const killTimeout = setTimeout(() => {
        if (this.process && !this.process.killed) {
          log('web-server', 'Force killing process...');
          this.process.kill('SIGKILL');
        }
        resolve();
      }, 5000);

      this.process!.on('exit', () => {
        clearTimeout(killTimeout);
        resolve();
      });

      this.process!.kill('SIGTERM');
    });
  }

  /**
   * Start health check polling
   */
  private startHealthCheck(): void {
    if (!this.config) return;
    
    let consecutiveFailures = 0;
    const maxFailures = 3;
    
    this.healthCheckInterval = setInterval(async () => {
      try {
        const response = await fetch(
          `http://localhost:${this.config!.port}/api/health`,
          { signal: AbortSignal.timeout(5000) }
        );
        
        if (response.ok) {
          consecutiveFailures = 0;
          const health = await response.json() as WebServerHealth;
          this.emit('health', health);
        } else {
          consecutiveFailures++;
        }
      } catch {
        consecutiveFailures++;
      }
      
      if (consecutiveFailures >= maxFailures) {
        logWarn('web-server', `Health check failed ${consecutiveFailures} times`);
        this.emit('unhealthy');
      }
    }, 10000);
  }

  private stopHealthCheck(): void {
    if (this.healthCheckInterval) {
      clearInterval(this.healthCheckInterval);
      this.healthCheckInterval = null;
    }
  }

  /**
   * Get the bootstrap result (credentials, host info)
   */
  getBootstrapResult(): BootstrapResult | null {
    return this.bootstrapResult;
  }

  /**
   * Get the current configuration
   */
  getConfig(): WebServerConfig | null {
    return this.config;
  }

  /**
   * Check if the server is running
   */
  isRunning(): boolean {
    return this.process !== null && !this.process.killed;
  }

  /**
   * Get local URL for the web server
   */
  getLocalUrl(): string | null {
    if (!this.config) return null;
    return `http://localhost:${this.config.port}`;
  }

  /**
   * Get remote URL (if available)
   */
  getRemoteUrl(): string | null {
    if (!this.bootstrapResult?.server?.external_ip) return null;
    if (!this.config) return null;
    return `http://${this.bootstrapResult.server.external_ip}:${this.config.port}`;
  }
}

// Export singleton
export const webServerManager = new WebServerManager();
```

### 2.2 Integration with Backlight App

```typescript
// src/main/index.ts - Add to existing startup sequence

import { webServerManager } from './lib/moonlight-web/WebServerManager';
import { settingsStorage } from './lib/settings/storage';

async function initializeApp() {
  // ... existing Sunshine initialization ...
  
  // Initialize web server after Sunshine is ready
  log('app', 'Initializing web server...');
  
  try {
    const bootstrapResult = await webServerManager.initialize();
    
    if (bootstrapResult.success) {
      // Store credentials securely
      const settings = await settingsStorage.load();
      settings.webServer = {
        username: bootstrapResult.user!.username,
        password: bootstrapResult.user!.password,
        port: bootstrapResult.server!.port,
        externalIp: bootstrapResult.server!.external_ip,
        upnpSuccess: bootstrapResult.server!.upnp_success,
        natType: bootstrapResult.server!.nat_type,
        hostId: bootstrapResult.host!.id,
        hostName: bootstrapResult.host!.name,
      };
      await settingsStorage.save(settings);
      
      log('app', 'Web server initialized successfully');
    } else {
      logError('app', 'Web server bootstrap failed:', bootstrapResult.error);
    }
  } catch (error) {
    logError('app', 'Failed to initialize web server:', error);
  }
  
  // ... rest of initialization ...
}

// Handle app quit
app.on('before-quit', async (event) => {
  event.preventDefault();
  
  try {
    await webServerManager.stop();
    // ... existing Sunshine shutdown ...
  } finally {
    app.exit(0);
  }
});
```

### 2.3 QR Code Generation

```typescript
// src/main/lib/pairing/qrCodeGenerator.ts

import { webServerManager } from '../moonlight-web/WebServerManager';
import { settingsStorage } from '../settings/storage';
import { getCurrentNetworkInfo } from '../network/ipDetection';

interface AndroidQRData {
  type: 'backlight-webserver';
  version: number;
  server: {
    localUrl: string;
    remoteUrl: string | null;
    remoteAvailable: boolean;
  };
  credentials: {
    username: string;
    password: string;
  };
  host: {
    id: number;
    name: string;
  };
}

export async function generateAndroidQRCode(): Promise<string> {
  const settings = await settingsStorage.load();
  const webServerSettings = settings.webServer;
  
  if (!webServerSettings) {
    throw new Error('Web server not configured');
  }
  
  const networkInfo = getCurrentNetworkInfo(webServerSettings.port);
  
  const qrData: AndroidQRData = {
    type: 'backlight-webserver',
    version: 1,
    server: {
      localUrl: `http://${networkInfo.ipAddress}:${webServerSettings.port}`,
      remoteUrl: webServerSettings.externalIp 
        ? `http://${webServerSettings.externalIp}:${webServerSettings.port}`
        : null,
      remoteAvailable: webServerSettings.upnpSuccess || !!webServerSettings.externalIp,
    },
    credentials: {
      username: webServerSettings.username,
      password: webServerSettings.password,
    },
    host: {
      id: webServerSettings.hostId,
      name: webServerSettings.hostName,
    },
  };
  
  return JSON.stringify(qrData);
}

// Existing iOS QR code generation remains unchanged
export { createPairingSession as generateIOSQRCode } from '../websocket/server';
```

### 2.4 IPC Handlers

```typescript
// src/main/ipc/handlers/WebServerHandler.ts

import { ipcMain } from 'electron';
import { webServerManager } from '../../lib/moonlight-web/WebServerManager';
import { generateAndroidQRCode } from '../../lib/pairing/qrCodeGenerator';

export function registerWebServerHandlers(): void {
  ipcMain.handle('webserver:get-status', async () => {
    const config = webServerManager.getConfig();
    const bootstrap = webServerManager.getBootstrapResult();
    
    return {
      running: webServerManager.isRunning(),
      port: config?.port,
      localUrl: webServerManager.getLocalUrl(),
      remoteUrl: webServerManager.getRemoteUrl(),
      hostPaired: bootstrap?.host?.paired ?? false,
      natType: bootstrap?.server?.nat_type,
      upnpSuccess: bootstrap?.server?.upnp_success,
    };
  });
  
  ipcMain.handle('webserver:generate-android-qr', async () => {
    return generateAndroidQRCode();
  });
  
  ipcMain.handle('webserver:get-remote-access-info', async () => {
    const bootstrap = webServerManager.getBootstrapResult();
    
    if (!bootstrap?.server) {
      return null;
    }
    
    return {
      externalIp: bootstrap.server.external_ip,
      port: bootstrap.server.port,
      upnpSuccess: bootstrap.server.upnp_success,
      natType: bootstrap.server.nat_type,
    };
  });
}
```

---

## 3. Web Server Modifications

### 3.1 Bootstrap Mode Implementation

Add to `moonlight-web/web-server/src/cli.rs`:

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "moonlight-web-server")]
#[command(about = "Moonlight Web Stream Server")]
pub struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "./server/config.json")]
    pub config: String,

    /// Override bind address
    #[arg(short, long)]
    pub bind: Option<String>,

    /// Override streamer path
    #[arg(short, long)]
    pub streamer: Option<String>,

    /// Log level
    #[arg(short, long, default_value = "Info")]
    pub log_level: String,

    /// Enable bootstrap mode for embedded deployment
    #[arg(long)]
    pub bootstrap: bool,

    /// Username for auto-created user (bootstrap mode)
    #[arg(long, default_value = "backlight")]
    pub bootstrap_user: String,

    /// Password for auto-created user (bootstrap mode)
    #[arg(long)]
    pub bootstrap_password: Option<String>,

    /// Host address to auto-add (bootstrap mode)
    #[arg(long, default_value = "localhost")]
    pub bootstrap_host: String,

    /// Host HTTP port (bootstrap mode)
    #[arg(long, default_value = "48989")]
    pub bootstrap_host_port: u16,
}
```

### 3.2 Bootstrap Logic

Add to `moonlight-web/web-server/src/main.rs`:

```rust
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
struct BootstrapResult {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<BootstrapUser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<BootstrapHost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<BootstrapServer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct BootstrapUser {
    id: u32,
    username: String,
    password: String,
}

#[derive(Serialize)]
struct BootstrapHost {
    id: u32,
    name: String,
    paired: bool,
}

#[derive(Serialize)]
struct BootstrapServer {
    port: u16,
    external_ip: Option<String>,
    upnp_success: bool,
    nat_type: String,
}

async fn run_bootstrap(
    app: &App,
    cli: &Cli,
    remote_info: Option<&RemoteAccessInfo>,
) -> BootstrapResult {
    // Generate password if not provided
    let password = cli.bootstrap_password
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Step 1: Create or get user
    let user = match app.get_or_create_user(&cli.bootstrap_user, &password).await {
        Ok(user) => user,
        Err(e) => {
            return BootstrapResult {
                success: false,
                error: Some(format!("Failed to create user: {}", e)),
                user: None,
                host: None,
                server: None,
            };
        }
    };

    // Step 2: Add localhost host if not exists
    let host_id = match app.get_or_add_host(
        user.id,
        &cli.bootstrap_host,
        cli.bootstrap_host_port,
    ).await {
        Ok(id) => id,
        Err(e) => {
            return BootstrapResult {
                success: false,
                error: Some(format!("Failed to add host: {}", e)),
                user: Some(BootstrapUser {
                    id: user.id,
                    username: cli.bootstrap_user.clone(),
                    password: password.clone(),
                }),
                host: None,
                server: None,
            };
        }
    };

    // Step 3: Auto-pair with host
    let paired = match app.auto_pair_host(user.id, host_id).await {
        Ok(()) => true,
        Err(e) => {
            log::warn!("Auto-pair failed (non-fatal): {}", e);
            false
        }
    };

    // Step 4: Get host name
    let host_name = app.get_host_name(host_id).await
        .unwrap_or_else(|_| "Gaming PC".to_string());

    // Build result
    let port = app.config().web_server.bind_address.port();
    
    BootstrapResult {
        success: true,
        user: Some(BootstrapUser {
            id: user.id,
            username: cli.bootstrap_user.clone(),
            password,
        }),
        host: Some(BootstrapHost {
            id: host_id.0,
            name: host_name,
            paired,
        }),
        server: Some(BootstrapServer {
            port,
            external_ip: remote_info.and_then(|r| r.external_ip.clone()),
            upnp_success: remote_info.map(|r| r.discovery_method == "upnp").unwrap_or(false),
            nat_type: remote_info.map(|r| r.nat_type.clone()).unwrap_or_else(|| "unknown".to_string()),
        }),
        error: None,
    }
}
```

### 3.3 Health Endpoint

Add to `moonlight-web/web-server/src/api/mod.rs`:

```rust
use actix_web::{get, web::Data, HttpResponse};
use serde::Serialize;
use std::time::Instant;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_secs: u64,
    active_streams: u32,
    hosts: HostsHealth,
    remote_access: RemoteAccessHealth,
}

#[derive(Serialize)]
struct HostsHealth {
    total: u32,
    paired: u32,
    online: u32,
}

#[derive(Serialize)]
struct RemoteAccessHealth {
    upnp_enabled: bool,
    upnp_success: bool,
    external_ip: Option<String>,
    nat_type: String,
}

static START_TIME: once_cell::sync::Lazy<Instant> = 
    once_cell::sync::Lazy::new(Instant::now);

#[get("/health")]
async fn health(
    app: Data<App>,
    remote_provider: Data<RemoteAccessProvider>,
) -> HttpResponse {
    let uptime = START_TIME.elapsed().as_secs();
    let remote_info = remote_provider.get_info();
    
    // Get host stats (simplified - expand as needed)
    let (total, paired, online) = app.get_host_stats().await.unwrap_or((0, 0, 0));
    
    let response = HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: uptime,
        active_streams: app.active_stream_count(),
        hosts: HostsHealth { total, paired, online },
        remote_access: RemoteAccessHealth {
            upnp_enabled: app.config().upnp.enabled,
            upnp_success: remote_info.as_ref()
                .map(|r| r.discovery_method == "upnp")
                .unwrap_or(false),
            external_ip: remote_info.as_ref().and_then(|r| r.external_ip.clone()),
            nat_type: remote_info.as_ref()
                .map(|r| r.nat_type.clone())
                .unwrap_or_else(|| "unknown".to_string()),
        },
    };
    
    HttpResponse::Ok().json(response)
}
```

---

## 4. Android Client Updates

### 4.1 QR Code Parsing

```kotlin
// QRCodeParser.kt

sealed class PairingData {
    data class WebServerPairing(
        val localUrl: String,
        val remoteUrl: String?,
        val remoteAvailable: Boolean,
        val username: String,
        val password: String,
        val hostId: Int,
        val hostName: String
    ) : PairingData()
    
    data class SunshinePairing(
        // Existing iOS pairing data
    ) : PairingData()
}

fun parseQRCode(data: String): PairingData {
    val json = JSONObject(data)
    
    return when (json.getString("type")) {
        "backlight-webserver" -> {
            val server = json.getJSONObject("server")
            val credentials = json.getJSONObject("credentials")
            val host = json.getJSONObject("host")
            
            PairingData.WebServerPairing(
                localUrl = server.getString("localUrl"),
                remoteUrl = server.optString("remoteUrl", null),
                remoteAvailable = server.getBoolean("remoteAvailable"),
                username = credentials.getString("username"),
                password = credentials.getString("password"),
                hostId = host.getInt("id"),
                hostName = host.getString("name")
            )
        }
        "fuji-pairing" -> {
            throw UnsupportedOperationException(
                "This QR code is for iOS devices. Please use the Android QR code."
            )
        }
        else -> throw IllegalArgumentException("Unknown QR code type")
    }
}
```

### 4.2 Credential Storage

```kotlin
// CredentialStore.kt

class CredentialStore(private val context: Context) {
    private val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    private val prefs = context.getSharedPreferences("web_server_auth", Context.MODE_PRIVATE)
    
    fun saveCredentials(username: String, password: String) {
        // Encrypt password with Android Keystore
        val encryptedPassword = encrypt(password)
        
        prefs.edit()
            .putString("username", username)
            .putString("password_encrypted", encryptedPassword)
            .apply()
    }
    
    fun getCredentials(): Pair<String, String>? {
        val username = prefs.getString("username", null) ?: return null
        val encryptedPassword = prefs.getString("password_encrypted", null) ?: return null
        
        val password = decrypt(encryptedPassword)
        return Pair(username, password)
    }
    
    fun saveServerUrls(localUrl: String, remoteUrl: String?) {
        prefs.edit()
            .putString("local_url", localUrl)
            .putString("remote_url", remoteUrl)
            .apply()
    }
    
    fun clearAll() {
        prefs.edit().clear().apply()
    }
    
    private fun encrypt(data: String): String {
        // Implementation using Android Keystore
        // ...
    }
    
    private fun decrypt(data: String): String {
        // Implementation using Android Keystore
        // ...
    }
}
```

### 4.3 API Client with Auto-Reconnect

```kotlin
// WebServerApiClient.kt

class WebServerApiClient(
    private val credentialStore: CredentialStore
) {
    private var sessionToken: String? = null
    private var baseUrl: String? = null
    
    suspend fun initialize(localUrl: String, remoteUrl: String?): Boolean {
        // Try remote URL first (if available and not on same network)
        val urls = listOfNotNull(remoteUrl, localUrl)
        
        for (url in urls) {
            try {
                baseUrl = url
                login()
                return true
            } catch (e: Exception) {
                Log.w(TAG, "Failed to connect to $url: ${e.message}")
            }
        }
        
        return false
    }
    
    private suspend fun login(): String {
        val (username, password) = credentialStore.getCredentials()
            ?: throw IllegalStateException("No credentials stored")
        
        val response = httpClient.post("$baseUrl/api/login") {
            contentType(ContentType.Application.Json)
            setBody(LoginRequest(username, password))
        }
        
        if (response.status != HttpStatusCode.OK) {
            throw AuthenticationException("Login failed: ${response.status}")
        }
        
        val loginResponse = response.body<LoginResponse>()
        sessionToken = loginResponse.sessionToken
        return sessionToken!!
    }
    
    suspend fun <T> authenticatedRequest(
        block: suspend HttpClient.() -> HttpResponse
    ): T {
        if (sessionToken == null) {
            login()
        }
        
        val response = httpClient.block()
        
        if (response.status == HttpStatusCode.Unauthorized) {
            // Session expired, re-authenticate
            Log.d(TAG, "Session expired, re-authenticating...")
            sessionToken = null
            login()
            
            // Retry request
            return httpClient.block().body()
        }
        
        return response.body()
    }
    
    suspend fun getHosts(): List<Host> = authenticatedRequest {
        get("$baseUrl/api/hosts") {
            bearerAuth(sessionToken!!)
        }
    }
    
    suspend fun getApps(hostId: Int): List<App> = authenticatedRequest {
        get("$baseUrl/api/apps") {
            bearerAuth(sessionToken!!)
            parameter("host_id", hostId)
        }
    }
    
    fun createStreamWebSocket(hostId: Int, appId: Int): WebSocket {
        return httpClient.webSocket(
            urlString = "$baseUrl/api/host/stream".replace("http", "ws"),
            request = {
                bearerAuth(sessionToken!!)
            }
        )
    }
}
```

---

## 5. Testing Strategy

### 5.1 Unit Tests

| Component | Test Cases |
|-----------|------------|
| WebServerManager | Port selection, config generation, process lifecycle |
| QR Code Generator | Schema validation, credential inclusion |
| Bootstrap Logic | User creation, host addition, auto-pairing |
| Health Endpoint | Response format, uptime calculation |

### 5.2 Integration Tests

| Scenario | Steps | Expected Result |
|----------|-------|-----------------|
| First Launch | Start Backlight → Web server initializes | User/host created, paired |
| Android Pairing | Scan QR → Login → Get hosts | Session established |
| Local Streaming | Start stream on same network | < 20ms latency |
| Remote Streaming | Start stream from different network | Connection established |
| Session Expiry | Wait 24h → Make request | Auto re-login |
| Crash Recovery | Kill web-server.exe → Wait | Auto restart within 5s |

### 5.3 NAT Traversal Tests

| NAT Type | Test Network | Expected Behavior |
|----------|--------------|-------------------|
| Full Cone | Home router | Direct P2P works |
| Restricted | Office network | UPnP may be needed |
| Symmetric | Mobile hotspot | Show guidance message |
| CGNAT | Carrier network | Show guidance message |

---

## 6. Troubleshooting Guide

### 6.1 Web Server Won't Start

**Symptoms:** Backlight starts but Android QR code not available

**Checks:**
1. Check if port 8080 is in use: `netstat -ano | findstr :8080`
2. Check web-server.exe exists in resources
3. Check firewall allows web-server.exe
4. Check logs at `%APPDATA%/Backlight/moonlight-web/logs/`

**Resolution:**
- Kill conflicting process
- Reinstall Backlight
- Manually add firewall rule

### 6.2 Auto-Pairing Fails

**Symptoms:** Bootstrap shows `paired: false`

**Checks:**
1. Is Sunshine running? Check port 48989
2. Is Sunshine's OTP endpoint working?
3. Check web server logs for pairing errors

**Resolution:**
- Restart Sunshine
- Check Sunshine credentials match expected (username:password)
- Manually pair via Sunshine web UI as fallback

### 6.3 Android Can't Connect

**Symptoms:** App shows "Connection failed"

**Checks:**
1. Same network? Try local URL
2. Different network? Check UPnP/port forward
3. Check NAT type in QR code data

**Resolution:**
- Ensure on same WiFi for local
- Configure port forwarding for remote
- Use VPN (Tailscale) for Symmetric NAT

### 6.4 Stream Quality Issues

**Symptoms:** High latency, stuttering

**Checks:**
1. Network bandwidth (need 20+ Mbps)
2. WebRTC stats in Android app
3. Check if using TURN (shouldn't be)

**Resolution:**
- Reduce bitrate setting
- Switch to 5GHz WiFi
- Close bandwidth-heavy apps

---

## Appendix: File Checklist

### New Files to Create in Backlight

```
src/main/lib/moonlight-web/
├── WebServerManager.ts
├── index.ts
└── types.ts

src/main/lib/pairing/
└── qrCodeGenerator.ts (modify existing or create)

src/main/ipc/handlers/
└── WebServerHandler.ts
```

### Files to Modify in Backlight

```
src/main/index.ts                 # Add web server initialization
src/main/app/App.ts               # Add shutdown handling
src/preload/index.ts              # Expose IPC methods
src/renderer/src/components/      # Add Android QR UI
```

### New Files to Create in moonlight-web-stream

```
moonlight-web/web-server/src/bootstrap.rs  # Bootstrap logic
```

### Files to Modify in moonlight-web-stream

```
moonlight-web/web-server/src/cli.rs    # Add bootstrap CLI args
moonlight-web/web-server/src/main.rs   # Call bootstrap logic
moonlight-web/web-server/src/api/mod.rs # Add health endpoint
```
