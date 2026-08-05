use crate::connection::CdpClient;
use crate::error::{XcelerateResult, XcelerateError};
use crate::page::Page;
use std::sync::Arc;
use tokio_tungstenite::connect_async;
use tokio::sync::mpsc;
use std::process::Stdio;
use std::path::PathBuf;
use std::time::Duration;

const CDC_PAYLOAD: &str = include_str!("cdc_payload.js");

/// Configuration for the Browser instance.
#[derive(uniffi::Record)]
pub struct BrowserConfig {
    /// Whether to run the browser in headless mode.
    pub headless: bool,
    /// Whether to apply stealth patches to the binary.
    pub stealth: bool,
    /// Whether to run the browser as a detached process.
    pub detached: bool,
    /// Optional path to the browser executable.
    pub executable_path: Option<String>,
    /// Optional directory where downloaded files should be saved.
    pub download_path: Option<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            headless: true,
            stealth: true,
            detached: true,
            executable_path: None,
            download_path: None,
        }
    }
}

/// Represents a browser instance (e.g., Chrome or Edge).
#[derive(uniffi::Object)]
pub struct Browser {
    pub(crate) client: Arc<CdpClient>,
    _process: tokio::sync::Mutex<Option<tokio::process::Child>>, 
    _process_guard: Option<crate::stealth::process::ProcessGuard>,
    _user_data_dir: Option<tempfile::TempDir>, 
    _stealth: bool,
}

#[uniffi::export(async_runtime = "tokio")]
impl Browser {
    #[uniffi::constructor]
    pub async fn launch(config: BrowserConfig) -> XcelerateResult<Arc<Self>> {
        let exe = match config.executable_path {
            Some(p) => Some(PathBuf::from(p)),
            None => find_chrome_executable(),
        }.ok_or_else(|| {
            XcelerateError::NotFound("Chrome executable not found. Please specify executable_path.".into())
        })?;

        // 1. Setup environment
        let user_data_dir = tempfile::tempdir().map_err(|_| XcelerateError::InternalError)?;
        let port = get_free_port().ok_or(XcelerateError::InternalError)?;

        let exe = if config.stealth {
            crate::stealth::patcher::BinaryPatcher::patch_to_temp(&exe)?
        } else {
            exe
        };


        // 2. Spawn process
        let mut cmd = std::process::Command::new(&exe);
        setup_browser_args(&mut cmd, &user_data_dir, port, config.headless);

        let (child, guard) = if config.detached {
            let pid = crate::stealth::process::spawn_detached(cmd)?;
            let guard = crate::stealth::process::ProcessGuard { pid, auto_kill: false };
            (None, Some(guard))
        } else {
            let mut t_cmd = tokio::process::Command::from(cmd);
            let child = t_cmd.spawn().map_err(|e| XcelerateError::NotFound(format!("Failed to start Chrome: {}", e)))?;
            let pid = child.id().ok_or(XcelerateError::InternalError)?;
            let guard = crate::stealth::process::ProcessGuard { pid, auto_kill: true };
            (Some(child), Some(guard))
        };

        // 3. Connect to debugger
        let ws_url = wait_for_ws_url(port).await?;
        
        let (ws, _) = connect_async(&ws_url).await?;
        
        let (tx, rx) = mpsc::unbounded_channel();
        let (handler, _event_rx) = crate::connection::CdpHandler::new(ws, rx);
        let client = Arc::new(CdpClient::new(tx, handler.event_tx.clone()));

        tokio::spawn(handler.run());

        // 4. Configure download behavior, if requested
        if let Some(download_path) = config.download_path {
            std::fs::create_dir_all(&download_path).map_err(|_| XcelerateError::InternalError)?;
            let params = browser_protocol::browser::SetDownloadBehaviorParams::builder("allow")
                .download_path(download_path)
                .build();
            let params_val = serde_json::to_value(&params)?;
            client.execute_raw(
                browser_protocol::browser::SetDownloadBehaviorParams::METHOD,
                None,
                params_val,
            ).await?;
        }

        Ok(Arc::new(Self {
            client, 
            _process: tokio::sync::Mutex::new(child),
            _process_guard: guard,
            _user_data_dir: Some(user_data_dir),
            _stealth: config.stealth,
        }))
    }

    pub async fn new_page(self: Arc<Self>, url: String) -> XcelerateResult<Arc<Page>> {
        // 1. Create target with about:blank so we can inject scripts before loading the real URL
        let create_target_params = browser_protocol::target::CreateTargetParams::builder("about:blank").build();
        let create_target_val = serde_json::to_value(&create_target_params)?;
        let target_raw = self.client.execute_raw(
            browser_protocol::target::CreateTargetParams::METHOD,
            None,
            create_target_val,
        ).await?;
        let target: browser_protocol::target::CreateTargetReturns = serde_json::from_value(target_raw)?;

        // 2. Attach to target
        let attach_params = browser_protocol::target::AttachToTargetParams::builder(target.target_id().clone())
            .flatten(true)
            .build();
        let attach_val = serde_json::to_value(&attach_params)?;
        let session_raw = self.client.execute_raw(
            browser_protocol::target::AttachToTargetParams::METHOD,
            None,
            attach_val,
        ).await?;
        let session: browser_protocol::target::AttachToTargetReturns = serde_json::from_value(session_raw)?;

        let page = Arc::new(Page {
            client: Arc::clone(&self.client),
            session_id: session.session_id().to_string(),
            mouse_x: std::sync::Mutex::new(100.0),
            mouse_y: std::sync::Mutex::new(100.0),
        });

        // 3. Inject stealth payload if enabled
        if self._stealth {
            page.add_script_to_evaluate_on_new_document(CDC_PAYLOAD.to_string()).await?;
            // We also need to enable the Page domain for some events to fire correctly
            let enable_val = serde_json::to_value(&browser_protocol::page::EnableParams::default())?;
            self.client.execute_raw(
                browser_protocol::page::EnableParams::METHOD,
                Some(&page.session_id),
                enable_val,
            ).await?;
        }

        // 4. Finally navigate to the actual URL
        page.navigate(url).await?;

        Ok(page)
    }

    /// Returns the browser version information.
    pub async fn version(&self) -> XcelerateResult<String> {
        let params_val = serde_json::to_value(&browser_protocol::browser::GetVersionParams {})?;
        let raw = self.client.execute_raw(browser_protocol::browser::GetVersionParams::METHOD, None, params_val).await?;
        let res: browser_protocol::browser::GetVersionReturns = serde_json::from_value(raw)?;
        Ok(format!("{} (Protocol {})", res.product(), res.protocol_version()))
    }

    /// Closes the browser and kills the process.
    pub async fn close(&self) -> XcelerateResult<()> {
        // Try to close gracefully via CDP first
        if let Ok(params_val) = serde_json::to_value(&browser_protocol::browser::CloseParams {}) {
            let _ = self.client.execute_raw(browser_protocol::browser::CloseParams::METHOD, None, params_val).await;
        }
        
        // Kill the process if it's still running
        let mut lock = self._process.lock().await;
        if let Some(mut child) = lock.take() {
            let _ = child.kill().await;
        }
        Ok(())
    }
}

fn find_chrome_executable() -> Option<PathBuf> {
    let paths = if cfg!(windows) {
        vec![
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ]
    } else {
        // Linux and others
        vec![
            "/usr/bin/google-chrome",
            "/usr/bin/chromium-browser",
            "/usr/bin/chromium",
            "/usr/bin/microsoft-edge-stable",
        ]
    };

    for path in paths {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Some(pb);
        }
    }
    None
}

fn get_free_port() -> Option<u16> {
    use std::net::TcpListener;
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .ok()
}

fn setup_browser_args(cmd: &mut std::process::Command, user_data_dir: &tempfile::TempDir, port: u16, headless: bool) {
    cmd.arg(format!("--remote-debugging-port={}", port))
       .arg("--remote-debugging-address=127.0.0.1")
       .arg(format!("--user-data-dir={}", user_data_dir.path().display()))
       .arg("--no-first-run")
       .arg("--no-default-browser-check")
       .arg("--remote-allow-origins=*") 
       .arg("--no-startup-window")
       .stdout(Stdio::null())
       .stderr(Stdio::null());

    if headless {
        cmd.arg("--headless=new");
        cmd.arg("--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36");
    }
}

async fn wait_for_ws_url(port: u16) -> XcelerateResult<String> {
    let version_url = format!("http://127.0.0.1:{}/json/version", port);
    
    let mut attempts = 0;
    loop {
        match reqwest::get(&version_url).await {
            Ok(resp) => {
                let json: serde_json::Value = resp.json().await.map_err(|_| XcelerateError::InternalError)?;
                if let Some(ws_url) = json["webSocketDebuggerUrl"].as_str() {
                    return Ok(ws_url.to_string());
                }
            }
            Err(_) => {
                attempts += 1;
                if attempts > 100 { 
                    return Err(XcelerateError::NotFound("Timed out waiting for Chrome HTTP server".into())); 
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
    }
}
