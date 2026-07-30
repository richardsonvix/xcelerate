use crate::connection::CdpClient;
use crate::element::Element;
use crate::error::{XcelerateResult, XcelerateError};
use std::sync::Arc;
use browser_protocol::page::{GetLayoutMetricsParams, GetLayoutMetricsReturns, CaptureScreenshotParams, CaptureScreenshotReturns, ReloadParams, NavigateParams, EnableParams};
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
        let selector_json = serde_json::to_string(&selector)?;
        let js = format!("document.querySelector({})", selector_json);

        // Evaluate returns complex JSON, we handle it internally
        let params = js_protocol::runtime::EvaluateParams::builder(js).build();
        let params_val = serde_json::to_value(&params)?;
        let raw = self.client.execute_raw(
            js_protocol::runtime::EvaluateParams::METHOD,
            Some(&self.session_id),
            params_val,
        ).await?;
        let result: js_protocol::runtime::EvaluateReturns = serde_json::from_value(raw)?;

        if let Some(obj_id) = result.result().object_id() {
            Ok(Arc::new(Element {
                page: self.clone(),
                object_id: obj_id.to_string(),
            }))
        } else {
            Err(XcelerateError::NotFound(selector))
        }
    }

    /// Finds all elements matching the CSS selector (equivalent to `document.querySelectorAll`).
    pub async fn find_elements(self: Arc<Self>, selector: String) -> XcelerateResult<Vec<Arc<Element>>> {
        let selector_json = serde_json::to_string(&selector)?;
        let js = format!("document.querySelectorAll({})", selector_json);

        // 1. Evaluate the querySelectorAll call to get a remote handle (objectId) to the NodeList.
        let eval_params = js_protocol::runtime::EvaluateParams::builder(js).build();
        let eval_params_val = serde_json::to_value(&eval_params)?;
        let raw = self.client.execute_raw(
            js_protocol::runtime::EvaluateParams::METHOD,
            Some(&self.session_id),
            eval_params_val,
        ).await?;
        let list_result: js_protocol::runtime::EvaluateReturns = serde_json::from_value(raw)?;

        let list_object_id = match list_result.result().object_id() {
            Some(id) => id.to_string(),
            None => return Ok(Vec::new()),
        };

        // 2. Enumerate the NodeList's own properties to resolve each entry's objectId.
        let get_props_params = js_protocol::runtime::GetPropertiesParams::builder(list_object_id.clone())
            .own_properties(true)
            .build();
        let get_props_val = serde_json::to_value(&get_props_params)?;
        let props_raw = self.client.execute_raw(
            js_protocol::runtime::GetPropertiesParams::METHOD,
            Some(&self.session_id),
            get_props_val,
        ).await;

        // The NodeList wrapper object itself is no longer needed once its entries are resolved.
        let release_params = js_protocol::runtime::ReleaseObjectParams::builder(list_object_id).build();
        let release_val = serde_json::to_value(&release_params)?;
        let _ = self.client.execute_raw(
            js_protocol::runtime::ReleaseObjectParams::METHOD,
            Some(&self.session_id),
            release_val,
        ).await;

        let props: js_protocol::runtime::GetPropertiesReturns = serde_json::from_value(props_raw?)?;

        let mut elements = Vec::new();
        for prop in props.result() {
            // Array-like indices are numeric strings ("0", "1", ...); skip "length" and other own properties.
            if prop.name().parse::<usize>().is_err() {
                continue;
            }
            if let Some(value) = prop.value() {
                if let Some(obj_id) = value.object_id() {
                    elements.push(Arc::new(Element {
                        page: self.clone(),
                        object_id: obj_id.to_string(),
                    }));
                }
            }
        }

        Ok(elements)
    }

    /// Executes an arbitrary JavaScript expression/script in the page's context and
    /// returns its result serialized as a JSON string.
    ///
    /// Runs in the same `Runtime.evaluate` context used internally (title/content/etc.),
    /// after the stealth payload has already been injected for this page, so it does not
    /// bypass or race the anti-detection setup.
    ///
    /// `timeout_ms` bounds how long we wait for the CDP response. Without it a script that
    /// blocks the page's JS event loop (e.g. `alert()`, or a promise that never settles since
    /// `awaitPromise` is always on) hangs this call forever, since `Runtime.evaluate` simply
    /// never replies until the loop is unblocked. Defaults to 30s when not provided.
    pub async fn evaluate(&self, script: String, timeout_ms: Option<u64>) -> XcelerateResult<String> {
        let params = js_protocol::runtime::EvaluateParams::builder(script)
            .return_by_value(true)
            .await_promise(true)
            .build();
        let params_val = serde_json::to_value(&params)?;
        let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));

        let raw = tokio::time::timeout(
            timeout,
            self.client.execute_raw(
                js_protocol::runtime::EvaluateParams::METHOD,
                Some(&self.session_id),
                params_val,
            ),
        )
        .await
        .map_err(|_| XcelerateError::Timeout("evaluate timed out".into()))??;
        let res: js_protocol::runtime::EvaluateReturns = serde_json::from_value(raw)?;

        if let Some(exception) = res.exception_details() {
            let message = exception.exception()
                .and_then(|e| e.description().map(|d| d.to_string()).or_else(|| e.value().map(|v| v.to_string())))
                .unwrap_or_else(|| exception.text().to_string());
            return Err(XcelerateError::CdpResponseError { code: 0, message });
        }

        Ok(res.result().value().map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()))
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
            let params = js_protocol::runtime::EvaluateParams::builder("document.readyState").build();
            let params_val = serde_json::to_value(&params)?;
            let raw = self.client.execute_raw(
                js_protocol::runtime::EvaluateParams::METHOD,
                Some(&self.session_id),
                params_val,
            ).await?;
            let res: js_protocol::runtime::EvaluateReturns = serde_json::from_value(raw)?;

            if res.result().value().is_some_and(|v| v.as_str() == Some("complete")) {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        Err(XcelerateError::NotFound("Navigation timeout".into()))
    }

    /// Reloads the page.
    pub async fn reload(&self) -> XcelerateResult<()> {
        let params = ReloadParams::default();
        let params_val = serde_json::to_value(&params)?;
        self.client.execute_raw(ReloadParams::METHOD, Some(&self.session_id), params_val).await?;
        Ok(())
    }

    /// Navigates to a URL.
    pub async fn navigate(&self, url: String) -> XcelerateResult<()> {
        let params = NavigateParams::builder(url).build();
        let params_val = serde_json::to_value(&params)?;
        self.client.execute_raw(NavigateParams::METHOD, Some(&self.session_id), params_val).await?;
        Ok(())
    }

    /// Returns the page title.
    pub async fn title(&self) -> XcelerateResult<String> {
        let params = js_protocol::runtime::EvaluateParams::builder("document.title").build();
        let params_val = serde_json::to_value(&params)?;
        let raw = self.client.execute_raw(
            js_protocol::runtime::EvaluateParams::METHOD,
            Some(&self.session_id),
            params_val,
        ).await?;
        let res: js_protocol::runtime::EvaluateReturns = serde_json::from_value(raw)?;
        Ok(res.result().value().and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }

    /// Returns the full HTML content of the page.
    pub async fn content(&self) -> XcelerateResult<String> {
        let params = js_protocol::runtime::EvaluateParams::builder("document.documentElement.outerHTML").build();
        let params_val = serde_json::to_value(&params)?;
        let raw = self.client.execute_raw(
            js_protocol::runtime::EvaluateParams::METHOD,
            Some(&self.session_id),
            params_val,
        ).await?;
        let res: js_protocol::runtime::EvaluateReturns = serde_json::from_value(raw)?;
        Ok(res.result().value().and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }

    pub async fn screenshot(&self) -> XcelerateResult<Vec<u8>> {
        let params = CaptureScreenshotParams::builder().build();
        let params_val = serde_json::to_value(&params)?;
        let raw = self.client.execute_raw(CaptureScreenshotParams::METHOD, Some(&self.session_id), params_val).await?;
        let res: CaptureScreenshotReturns = serde_json::from_value(raw)?;
        self.decode_base64(res.data().to_string())
    }

    pub async fn screenshot_full(&self) -> XcelerateResult<Vec<u8>> {
        let enable_val = serde_json::to_value(&EnableParams::default())?;
        let _ = self.client.execute_raw(EnableParams::METHOD, Some(&self.session_id), enable_val).await?;

        let metrics_val = serde_json::to_value(&GetLayoutMetricsParams {})?;
        let metrics_raw = self.client.execute_raw(GetLayoutMetricsParams::METHOD, Some(&self.session_id), metrics_val).await?;
        let metrics: GetLayoutMetricsReturns = serde_json::from_value(metrics_raw)?;

        let width = metrics.content_size().width() as u64;
        let height = metrics.content_size().height() as i64;

        let override_params = SetDeviceMetricsOverrideParams::builder(width, height, 1.0, false).build();
        let override_val = serde_json::to_value(&override_params)?;
        self.client.execute_raw(SetDeviceMetricsOverrideParams::METHOD, Some(&self.session_id), override_val).await?;

        let screenshot_val = serde_json::to_value(&CaptureScreenshotParams::builder().build())?;
        let screenshot_raw = self.client.execute_raw(CaptureScreenshotParams::METHOD, Some(&self.session_id), screenshot_val).await?;
        let res: CaptureScreenshotReturns = serde_json::from_value(screenshot_raw)?;

        let clear_val = serde_json::to_value(&ClearDeviceMetricsOverrideParams {})?;
        let _ = self.client.execute_raw(ClearDeviceMetricsOverrideParams::METHOD, Some(&self.session_id), clear_val).await?;

        self.decode_base64(res.data().to_string())
    }

    pub async fn pdf(&self) -> XcelerateResult<Vec<u8>> {
        let params_val = serde_json::to_value(&browser_protocol::page::PrintToPDFParams::builder().build())?;
        let raw = self.client.execute_raw(browser_protocol::page::PrintToPDFParams::METHOD, Some(&self.session_id), params_val).await?;
        let res: browser_protocol::page::PrintToPDFReturns = serde_json::from_value(raw)?;
        self.decode_base64(res.data().to_string())
    }

    /// Evaluates a script on every new document.
    pub async fn add_script_to_evaluate_on_new_document(&self, source: String) -> XcelerateResult<String> {
        let params = browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams::builder(source).build();
        let params_val = serde_json::to_value(&params)?;
        let raw = self.client.execute_raw(
            browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams::METHOD,
            Some(&self.session_id),
            params_val,
        ).await?;
        let res: browser_protocol::page::AddScriptToEvaluateOnNewDocumentReturns = serde_json::from_value(raw)?;
        Ok(res.identifier().to_string())
    }

    pub async fn go_back(&self) -> XcelerateResult<()> {
        let params_val = serde_json::to_value(&js_protocol::runtime::EvaluateParams::builder("window.history.back()").build())?;
        let _ = self.client.execute_raw(js_protocol::runtime::EvaluateParams::METHOD, Some(&self.session_id), params_val).await?;
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

            let params_val = serde_json::to_value(
                &browser_protocol::input::DispatchMouseEventParams::builder("mouseMoved", curr_x, curr_y).build()
            )?;

            self.client.execute_raw(
                browser_protocol::input::DispatchMouseEventParams::METHOD,
                Some(&self.session_id),
                params_val,
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
        let params_val = serde_json::to_value(
            &browser_protocol::input::DispatchMouseEventParams::builder("mousePressed", cx, cy)
                .button(btn)
                .click_count(1)
                .build()
        )?;
        self.client.execute_raw(
            browser_protocol::input::DispatchMouseEventParams::METHOD,
            Some(&self.session_id),
            params_val,
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
        let params_val = serde_json::to_value(
            &browser_protocol::input::DispatchMouseEventParams::builder("mouseReleased", cx, cy)
                .button(btn)
                .click_count(1)
                .build()
        )?;
        self.client.execute_raw(
            browser_protocol::input::DispatchMouseEventParams::METHOD,
            Some(&self.session_id),
            params_val,
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
