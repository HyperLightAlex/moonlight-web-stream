//! Streamer Process Manager
//!
//! Tracks active streamer processes and provides cleanup for orphaned processes.
//! Ensures only one streamer process runs at a time and cleans up zombies.

use log::{debug, info, warn, error};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    process::Child,
    sync::Mutex,
    spawn,
    time::interval,
};

#[cfg(target_os = "windows")]
use std::process::Command as StdCommand;

/// Cleanup interval for checking zombie entries in our tracking map
const CLEANUP_INTERVAL_SECS: u64 = 60;

/// Information about a tracked streamer process
#[derive(Debug)]
pub struct StreamerProcessInfo {
    /// Process ID
    pub pid: u32,
    /// When the process was started
    pub started_at: Instant,
    /// Session ID associated with this process (if any)
    pub session_id: Option<String>,
    /// Whether the process is still active (updated on checks)
    pub active: bool,
}

/// Manager for streamer processes
#[derive(Debug)]
pub struct StreamerProcessManager {
    /// Active processes keyed by PID
    processes: Arc<Mutex<HashMap<u32, StreamerProcessInfo>>>,
    /// Path to the streamer executable (for killing by name)
    streamer_path: Arc<Mutex<Option<String>>>,
}

impl Default for StreamerProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamerProcessManager {
    /// Create a new streamer process manager
    pub fn new() -> Self {
        let processes = Arc::new(Mutex::new(HashMap::new()));
        
        // Start background cleanup task
        let processes_cleanup = processes.clone();
        spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
            loop {
                cleanup_interval.tick().await;
                Self::do_cleanup(&processes_cleanup).await;
            }
        });

        Self {
            processes,
            streamer_path: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the streamer executable path (used for orphan detection)
    pub async fn set_streamer_path(&self, path: String) {
        let mut streamer_path = self.streamer_path.lock().await;
        *streamer_path = Some(path);
    }

    /// Register a new streamer process
    pub async fn register(&self, child: &Child, session_id: Option<String>) {
        if let Some(pid) = child.id() {
            let info = StreamerProcessInfo {
                pid,
                started_at: Instant::now(),
                session_id: session_id.clone(),
                active: true,
            };
            
            let mut processes = self.processes.lock().await;
            processes.insert(pid, info);
            
            info!(
                "[StreamerManager] Registered streamer process PID {} for session {:?}",
                pid,
                session_id
            );
        } else {
            warn!("[StreamerManager] Could not get PID for new streamer process");
        }
    }

    /// Unregister a streamer process (called after successful kill)
    pub async fn unregister(&self, pid: u32) {
        let mut processes = self.processes.lock().await;
        if processes.remove(&pid).is_some() {
            info!("[StreamerManager] Unregistered streamer process PID {}", pid);
        }
    }

    /// Kill and unregister a process by PID
    pub async fn kill_process(&self, pid: u32) -> bool {
        info!("[StreamerManager] Attempting to kill streamer process PID {}", pid);
        
        let success = Self::kill_pid(pid).await;
        
        if success {
            self.unregister(pid).await;
        }
        
        success
    }

    /// Kill all tracked processes (used for cleanup before new stream)
    pub async fn kill_all_tracked(&self) {
        let pids: Vec<u32> = {
            let processes = self.processes.lock().await;
            processes.keys().copied().collect()
        };

        if pids.is_empty() {
            debug!("[StreamerManager] No tracked processes to kill");
            return;
        }

        info!("[StreamerManager] Killing {} tracked processes", pids.len());
        
        for pid in pids {
            self.kill_process(pid).await;
        }
    }

    /// Kill all streamer processes (tracked and orphaned) before starting a new stream
    /// This is the main entry point for cleanup before a new session
    pub async fn cleanup_before_new_session(&self) {
        info!("[StreamerManager] Cleaning up before new streaming session...");
        
        // First kill all tracked processes
        self.kill_all_tracked().await;
        
        // Then scan for orphaned streamer.exe processes
        self.kill_orphaned_streamers().await;
        
        info!("[StreamerManager] Pre-session cleanup complete");
    }

    /// Scan for and kill orphaned streamer.exe processes
    /// These are processes that exist but aren't tracked (e.g., from a previous crash)
    pub async fn kill_orphaned_streamers(&self) {
        #[cfg(target_os = "windows")]
        {
            // Get list of all streamer.exe processes using tasklist
            let output = StdCommand::new("tasklist")
                .args(["/FI", "IMAGENAME eq streamer.exe", "/FO", "CSV", "/NH"])
                .output();

            match output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let tracked_pids: std::collections::HashSet<u32> = {
                        let processes = self.processes.lock().await;
                        processes.keys().copied().collect()
                    };
                    
                    let mut orphan_count = 0;
                    
                    for line in stdout.lines() {
                        // CSV format: "streamer.exe","1234","Console",...
                        if line.starts_with("\"streamer.exe\"") {
                            let parts: Vec<&str> = line.split(',').collect();
                            if parts.len() >= 2 {
                                // Extract PID from quoted string
                                let pid_str = parts[1].trim_matches('"');
                                if let Ok(pid) = pid_str.parse::<u32>() {
                                    // Only kill if not tracked
                                    if !tracked_pids.contains(&pid) {
                                        warn!(
                                            "[StreamerManager] Found orphaned streamer process PID {}, killing...",
                                            pid
                                        );
                                        Self::kill_pid(pid).await;
                                        orphan_count += 1;
                                    }
                                }
                            }
                        }
                    }
                    
                    if orphan_count > 0 {
                        info!(
                            "[StreamerManager] Killed {} orphaned streamer processes",
                            orphan_count
                        );
                    } else {
                        debug!("[StreamerManager] No orphaned streamer processes found");
                    }
                }
                Err(e) => {
                    warn!("[StreamerManager] Failed to list processes: {:?}", e);
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // On Unix, use pkill or pgrep
            let output = std::process::Command::new("pgrep")
                .arg("-f")
                .arg("streamer")
                .output();

            match output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let tracked_pids: std::collections::HashSet<u32> = {
                        let processes = self.processes.lock().await;
                        processes.keys().copied().collect()
                    };
                    
                    for line in stdout.lines() {
                        if let Ok(pid) = line.trim().parse::<u32>() {
                            if !tracked_pids.contains(&pid) {
                                warn!(
                                    "[StreamerManager] Found orphaned streamer process PID {}, killing...",
                                    pid
                                );
                                Self::kill_pid(pid).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!("[StreamerManager] pgrep not available or failed: {:?}", e);
                }
            }
        }
    }

    /// Internal cleanup: remove zombie entries from our tracking map
    /// (processes that have exited but are still in our map)
    /// NOTE: We do NOT kill long-running tracked processes - they are legitimate streaming sessions!
    async fn do_cleanup(processes: &Arc<Mutex<HashMap<u32, StreamerProcessInfo>>>) {
        let mut dead_pids = Vec::new();

        {
            let processes_lock = processes.lock().await;
            for (pid, _info) in processes_lock.iter() {
                // Only remove entries for processes that are no longer running
                // (zombie entries from processes that crashed or were killed externally)
                if !Self::is_process_running(*pid).await {
                    debug!(
                        "[StreamerManager] Process {} is no longer running, removing from tracking",
                        pid
                    );
                    dead_pids.push(*pid);
                }
            }
        }

        // Remove dead entries from our tracking map
        if !dead_pids.is_empty() {
            let mut processes_lock = processes.lock().await;
            for pid in dead_pids {
                processes_lock.remove(&pid);
                debug!("[StreamerManager] Removed dead process {} from tracking", pid);
            }
        }
    }

    /// Check if a process is still running
    async fn is_process_running(pid: u32) -> bool {
        #[cfg(target_os = "windows")]
        {
            let output = StdCommand::new("tasklist")
                .args(["/FI", &format!("PID eq {}", pid), "/NH"])
                .output();
            
            match output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    stdout.contains(&pid.to_string())
                }
                Err(_) => false,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // On Unix, check /proc/{pid} or use kill -0
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
    }

    /// Kill a process by PID
    async fn kill_pid(pid: u32) -> bool {
        #[cfg(target_os = "windows")]
        {
            let output = StdCommand::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .output();
            
            match output {
                Ok(output) => {
                    if output.status.success() {
                        info!("[StreamerManager] Successfully killed process {}", pid);
                        true
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        // Check if process doesn't exist (already dead)
                        if stderr.contains("not found") || stderr.contains("not running") {
                            debug!("[StreamerManager] Process {} already dead", pid);
                            true
                        } else {
                            error!(
                                "[StreamerManager] Failed to kill process {}: {}",
                                pid, stderr
                            );
                            false
                        }
                    }
                }
                Err(e) => {
                    error!("[StreamerManager] Error running taskkill: {:?}", e);
                    false
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // On Unix, use kill signal
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            
            match kill(Pid::from_raw(pid as i32), Signal::SIGKILL) {
                Ok(_) => {
                    info!("[StreamerManager] Successfully killed process {}", pid);
                    true
                }
                Err(e) => {
                    // ESRCH means process doesn't exist (already dead)
                    if e == nix::errno::Errno::ESRCH {
                        debug!("[StreamerManager] Process {} already dead", pid);
                        true
                    } else {
                        error!("[StreamerManager] Failed to kill process {}: {:?}", pid, e);
                        false
                    }
                }
            }
        }
    }

    /// Get count of tracked processes
    pub async fn process_count(&self) -> usize {
        let processes = self.processes.lock().await;
        processes.len()
    }

    /// Check if any streamer is currently running
    pub async fn has_active_streamer(&self) -> bool {
        let processes = self.processes.lock().await;
        !processes.is_empty()
    }
}

// Global singleton
lazy_static::lazy_static! {
    pub static ref STREAMER_MANAGER: StreamerProcessManager = StreamerProcessManager::new();
}

/// Get the global streamer manager instance
pub fn streamer_manager() -> &'static StreamerProcessManager {
    &STREAMER_MANAGER
}
