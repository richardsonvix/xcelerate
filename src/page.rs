use crate::connection::CdpClient;
use crate::element::Element;
use crate::error::{XcelerateResult, XcelerateError};
use std::sync::Arc;
use browser_protocol::page::{GetLayoutMetricsParams, CaptureScreenshotParams, ReloadParams, NavigateParams, EnableParams};
use browser_protocol::emulation::{SetDeviceMetricsOverrideParams, ClearDeviceMetricsOverrideParams};

pub(crate) struct Lcg {
    state: u64,
}

impl Lcg {
    pub(crate) fn new() -> Self {
        use std::time::SystemTime;
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.state
    }

    fn next_f64(&mut self) -> f64 {
        let val = self.next();
        (val as f64) / (u64::MAX as f64)
    }

    pub(crate) fn range(&mut self, min: f64, max: f64) -> f64 {
        min + self.next_f64() * (max - min)
    }
}

#[derive(uniffi::Object)]
pub struct Page {
    pub(crate) client: Arc<CdpClient>,
    pub(crate) session_id: String,
    pub(crate) mouse_x: std::sync::Mutex<f64>,
    pub(crate) mouse_y: std::sync::Mutex<f64>,
}

#[uniffi::export(async_runtime = "tokio")]
impl Page {
    /// Finds an element matching the CSS selector.
    pub async fn find_element(self: Arc<Self>, selector: String) -> XcelerateResult<Arc<Element>> {
        let js = format!("document.querySelector('{}')", selector);
        
        // Evaluate returns complex JSON, we handle it internally
        self.client.execute_with_session(
            Some(&self.session_id),
            js_protocol::runtime::EvaluateParams {
                expression: js,
                ..Default::default()
            }
        ).await.and_then(|result| {
            if let Some(obj_id) = result.result.objectId {
                Ok(Arc::new(Element {
                    page: self.clone(),
                    object_id: obj_id,
                }))
            } else {
                Err(XcelerateError::NotFound(selector))
            }
        })
    }

    /// Waits for an element matching the selector to appear in the DOM.
    pub async fn wait_for_selector(self: Arc<Self>, selector: String) -> XcelerateResult<Arc<Element>> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(30);

        while start.elapsed() < timeout {
            if let Ok(element) = self.clone().find_element(selector.clone()).await {
                return Ok(element);
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        Err(XcelerateError::NotFound(format!("Timeout waiting for selector: {}", selector)))
    }

    /// Waits for the page to finish loading.
    pub async fn wait_for_navigation(&self) -> XcelerateResult<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(30);

        while start.elapsed() < timeout {
            // Internal call to evaluate
            let res = self.client.execute_with_session(
                Some(&self.session_id),
                js_protocol::runtime::EvaluateParams {
                    expression: "document.readyState".into(),
                    ..Default::default()
                }
            ).await?;
            
            if res.result.value.is_some_and(|v| v.as_str() == Some("complete")) {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        Err(XcelerateError::NotFound("Navigation timeout".into()))
    }

    /// Reloads the page.
    pub async fn reload(&self) -> XcelerateResult<()> {
        self.client.execute_with_session(
            Some(&self.session_id),
            ReloadParams { ..Default::default() }
        ).await.map(|_| ())
    }

    /// Navigates to a URL.
    pub async fn navigate(&self, url: String) -> XcelerateResult<()> {
        self.client.execute_with_session(
            Some(&self.session_id),
            NavigateParams { 
                url, 
                ..Default::default() 
            }
        ).await.map(|_| ())
    }

    /// Returns the page title.
    pub async fn title(&self) -> XcelerateResult<String> {
        let res = self.client.execute_with_session(
            Some(&self.session_id),
            js_protocol::runtime::EvaluateParams {
                expression: "document.title".into(),
                ..Default::default()
            }
        ).await?;
        Ok(res.result.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }

    /// Returns the full HTML content of the page.
    pub async fn content(&self) -> XcelerateResult<String> {
        let res = self.client.execute_with_session(
            Some(&self.session_id),
            js_protocol::runtime::EvaluateParams {
                expression: "document.documentElement.outerHTML".into(),
                ..Default::default()
            }
        ).await?;
        Ok(res.result.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }

    pub async fn screenshot(&self) -> XcelerateResult<Vec<u8>> {
        let res = self.client.execute_with_session(
            Some(&self.session_id),
            CaptureScreenshotParams { ..Default::default() }
        ).await?;
        self.decode_base64(res.data)
    }

    pub async fn screenshot_full(&self) -> XcelerateResult<Vec<u8>> {
        let _ = self.client.execute_with_session(
            Some(&self.session_id),
            EnableParams { ..Default::default() }
        ).await?;

        let metrics = self.client.execute_with_session(
            Some(&self.session_id),
            GetLayoutMetricsParams {}
        ).await?;

        let width = metrics.contentSize.width as u64;
        let height = metrics.contentSize.height as i64;

        let mut params = SetDeviceMetricsOverrideParams { ..Default::default() };
        params.width = width;
        params.height = height;
        params.deviceScaleFactor = 1.0;
        params.mobile = false;

        self.client.execute_with_session(
            Some(&self.session_id),
            params
        ).await?;

        let res = self.client.execute_with_session(
            Some(&self.session_id),
            CaptureScreenshotParams { ..Default::default() }
        ).await?;

        let _ = self.client.execute_with_session(
            Some(&self.session_id),
            ClearDeviceMetricsOverrideParams {}
        ).await?;

        self.decode_base64(res.data)
    }

    pub async fn pdf(&self) -> XcelerateResult<Vec<u8>> {
        let res = self.client.execute_with_session(
            Some(&self.session_id),
            browser_protocol::page::PrintToPDFParams { ..Default::default() }
        ).await?;
        self.decode_base64(res.data)
    }

    /// Evaluates a script on every new document.
    pub async fn add_script_to_evaluate_on_new_document(&self, source: String) -> XcelerateResult<String> {
        let res = self.client.execute_with_session(
            Some(&self.session_id),
            browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams {
                source,
                ..Default::default()
            }
        ).await?;
        Ok(res.identifier)
    }

    pub async fn go_back(&self) -> XcelerateResult<()> {
        let _ = self.client.execute_with_session(
            Some(&self.session_id),
            js_protocol::runtime::EvaluateParams {
                expression: "window.history.back()".into(),
                ..Default::default()
            }
        ).await?;
        Ok(())
    }

    fn decode_base64(&self, data: String) -> XcelerateResult<Vec<u8>> {
        use base64::{Engine as _, engine::general_purpose};
        general_purpose::STANDARD.decode(data).map_err(|e| XcelerateError::SerdeError(format!("Base64 decode failed: {}", e)))
    }

    /// Moves the mouse cursor from the current position to the target (x, y) along a realistic Bezier curve.
    pub async fn move_mouse(self: Arc<Self>, x: f64, y: f64) -> XcelerateResult<Arc<Self>> {
        let (start_x, start_y) = {
            let cx = *self.mouse_x.lock().unwrap();
            let cy = *self.mouse_y.lock().unwrap();
            (cx, cy)
        };

        let dx = x - start_x;
        let dy = y - start_y;
        let distance = (dx * dx + dy * dy).sqrt();

        if distance < 1.0 {
            {
                let mut cx = self.mouse_x.lock().unwrap();
                let mut cy = self.mouse_y.lock().unwrap();
                *cx = x;
                *cy = y;
            }
            return Ok(self);
        }

        let mut rng = Lcg::new();
        let (px, py) = if distance > 0.0 {
            (-dy / distance, dx / distance)
        } else {
            (0.0, 0.0)
        };

        let offset_scale1 = rng.range(-0.2, 0.2) * distance;
        let offset_scale2 = rng.range(-0.2, 0.2) * distance;

        let p1_x = start_x + dx * 0.25 + px * offset_scale1;
        let p1_y = start_y + dy * 0.25 + py * offset_scale1;

        let p2_x = start_x + dx * 0.75 + px * offset_scale2;
        let p2_y = start_y + dy * 0.75 + py * offset_scale2;

        let steps_f = (distance / rng.range(12.0, 25.0)).clamp(12.0, 60.0);
        let steps = steps_f as usize;

        for i in 1..=steps {
            let s = (i as f64) / (steps as f64);

            let t = if s < 0.5 {
                4.0 * s * s * s
            } else {
                let f = -2.0 * s + 2.0;
                1.0 - f * f * f / 2.0
            };

            let mt = 1.0 - t;
            let mt2 = mt * mt;
            let mt3 = mt2 * mt;
            let t2 = t * t;
            let t3 = t2 * t;

            let mut curr_x = mt3 * start_x + 3.0 * mt2 * t * p1_x + 3.0 * mt * t2 * p2_x + t3 * x;
            let mut curr_y = mt3 * start_y + 3.0 * mt2 * t * p1_y + 3.0 * mt * t2 * p2_y + t3 * y;

            if i < steps {
                curr_x += rng.range(-0.4, 0.4);
                curr_y += rng.range(-0.4, 0.4);
            }

            let params = browser_protocol::input::DispatchMouseEventParams {
                type_: "mouseMoved".into(),
                x: curr_x,
                y: curr_y,
                ..Default::default()
            };

            self.client.execute_with_session(
                Some(&self.session_id),
                params
            ).await?;

            let delay_ms = rng.range(6.0, 14.0) as u64;
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        }

        {
            let mut cx = self.mouse_x.lock().unwrap();
            let mut cy = self.mouse_y.lock().unwrap();
            *cx = x;
            *cy = y;
        }

        Ok(self)
    }

    /// Triggers a mousePress event at the current mouse coordinates.
    pub async fn mouse_down(self: Arc<Self>, button: String) -> XcelerateResult<Arc<Self>> {
        let (cx, cy) = {
            let x = *self.mouse_x.lock().unwrap();
            let y = *self.mouse_y.lock().unwrap();
            (x, y)
        };
        let btn = match button.as_str() {
            "left" => browser_protocol::input::MouseButton::Left,
            "right" => browser_protocol::input::MouseButton::Right,
            "middle" => browser_protocol::input::MouseButton::Middle,
            "back" => browser_protocol::input::MouseButton::Back,
            "forward" => browser_protocol::input::MouseButton::Forward,
            _ => browser_protocol::input::MouseButton::None,
        };
        let params = browser_protocol::input::DispatchMouseEventParams {
            type_: "mousePressed".into(),
            x: cx,
            y: cy,
            button: Some(btn),
            clickCount: Some(1),
            ..Default::default()
        };
        self.client.execute_with_session(
            Some(&self.session_id),
            params
        ).await?;
        Ok(self)
    }

    /// Triggers a mouseReleased event at the current mouse coordinates.
    pub async fn mouse_up(self: Arc<Self>, button: String) -> XcelerateResult<Arc<Self>> {
        let (cx, cy) = {
            let x = *self.mouse_x.lock().unwrap();
            let y = *self.mouse_y.lock().unwrap();
            (x, y)
        };
        let btn = match button.as_str() {
            "left" => browser_protocol::input::MouseButton::Left,
            "right" => browser_protocol::input::MouseButton::Right,
            "middle" => browser_protocol::input::MouseButton::Middle,
            "back" => browser_protocol::input::MouseButton::Back,
            "forward" => browser_protocol::input::MouseButton::Forward,
            _ => browser_protocol::input::MouseButton::None,
        };
        let params = browser_protocol::input::DispatchMouseEventParams {
            type_: "mouseReleased".into(),
            x: cx,
            y: cy,
            button: Some(btn),
            clickCount: Some(1),
            ..Default::default()
        };
        self.client.execute_with_session(
            Some(&self.session_id),
            params
        ).await?;
        Ok(self)
    }

    /// Moves the mouse to (x, y) and performs a click (down & up) with human-like delays.
    pub async fn click_mouse(self: Arc<Self>, x: f64, y: f64) -> XcelerateResult<Arc<Self>> {
        let page = self.clone();
        page.clone().move_mouse(x, y).await?;

        let mut rng = Lcg::new();
        let latency_delay = rng.range(50.0, 130.0) as u64;
        tokio::time::sleep(tokio::time::Duration::from_millis(latency_delay)).await;

        page.clone().mouse_down("left".to_string()).await?;

        let hold_delay = rng.range(60.0, 140.0) as u64;
        tokio::time::sleep(tokio::time::Duration::from_millis(hold_delay)).await;

        page.clone().mouse_up("left".to_string()).await?;

        Ok(self)
    }
}
