use crate::page::{Page, Lcg};
use crate::error::XcelerateResult;
use std::sync::Arc;

/// Represents an HTML element in the DOM.
#[derive(uniffi::Object)]
pub struct Element {
    pub(crate) page: Arc<Page>,
    pub(crate) object_id: String,
}

#[uniffi::export(async_runtime = "tokio")]
impl Element {
    /// Clicks the element.
    pub async fn click(self: Arc<Self>) -> XcelerateResult<Arc<Self>> {
        self.call_js("function() { this.click(); }".to_string()).await?;
        Ok(self)
    }

    pub async fn type_text(self: Arc<Self>, text: String) -> XcelerateResult<Arc<Self>> {
        // 1. Focus the element first
        self.clone().focus().await?;

        // 2. Dispatch key events for each character
        for c in text.chars() {
            let params = browser_protocol::input::DispatchKeyEventParams::builder("char")
                .text(c.to_string())
                .unmodified_text(c.to_string())
                .build();
            let params_val = serde_json::to_value(&params)?;

            self.page.client.execute_raw(
                browser_protocol::input::DispatchKeyEventParams::METHOD,
                Some(&self.page.session_id),
                params_val,
            ).await?;

            // Subtle delay to mimic human typing
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        Ok(self)
    }

    /// Hovers over the element.
    pub async fn hover(self: Arc<Self>) -> XcelerateResult<Arc<Self>> {
        self.call_js("function() { this.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })); }".to_string()).await?;
        Ok(self)
    }

    /// Clicks the element using realistic mouse movement and CDP input events.
    pub async fn click_stealth(self: Arc<Self>) -> XcelerateResult<Arc<Self>> {
        let js = "function() {
            this.scrollIntoView({ block: 'center', inline: 'center' });
            const rect = this.getBoundingClientRect();
            return JSON.stringify({
                x: rect.left,
                y: rect.top,
                width: rect.width,
                height: rect.height
            });
        }".to_string();

        let res = self.call_js(js).await?;
        let val_str = res.result().value()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| crate::error::XcelerateError::InternalError)?;

        #[derive(serde::Deserialize)]
        struct Rect {
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        }

        let rect: Rect = serde_json::from_str(&val_str)
            .map_err(|e| crate::error::XcelerateError::SerdeError(e.to_string()))?;

        let mut rng = Lcg::new();
        let target_x = rect.x + rect.width * 0.15 + rng.range(0.0, rect.width * 0.7);
        let target_y = rect.y + rect.height * 0.15 + rng.range(0.0, rect.height * 0.7);

        self.page.clone().click_mouse(target_x, target_y).await?;

        Ok(self)
    }

    /// Hovers over the element using realistic mouse movement.
    pub async fn hover_stealth(self: Arc<Self>) -> XcelerateResult<Arc<Self>> {
        let js = "function() {
            this.scrollIntoView({ block: 'center', inline: 'center' });
            const rect = this.getBoundingClientRect();
            return JSON.stringify({
                x: rect.left,
                y: rect.top,
                width: rect.width,
                height: rect.height
            });
        }".to_string();

        let res = self.call_js(js).await?;
        let val_str = res.result().value()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| crate::error::XcelerateError::InternalError)?;

        #[derive(serde::Deserialize)]
        struct Rect {
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        }

        let rect: Rect = serde_json::from_str(&val_str)
            .map_err(|e| crate::error::XcelerateError::SerdeError(e.to_string()))?;

        let mut rng = Lcg::new();
        let target_x = rect.x + rect.width * 0.15 + rng.range(0.0, rect.width * 0.7);
        let target_y = rect.y + rect.height * 0.15 + rng.range(0.0, rect.height * 0.7);

        self.page.clone().move_mouse(target_x, target_y).await?;

        Ok(self)
    }

    /// Focuses the element.
    pub async fn focus(self: Arc<Self>) -> XcelerateResult<Arc<Self>> {
        self.call_js("function() { this.focus(); }".to_string()).await?;
        Ok(self)
    }

    /// Returns the visible text of the element.
    pub async fn text(&self) -> XcelerateResult<String> {
        let res = self.call_js("function() { return this.innerText; }".to_string()).await?;
        Ok(res.result().value().and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }

    /// Returns the value of a specific attribute.
    pub async fn attribute(&self, name: String) -> XcelerateResult<Option<String>> {
        let js = format!("function() {{ return this.getAttribute('{}'); }}", name);
        let res = self.call_js(js).await?;
        Ok(res.result().value().and_then(|v| v.as_str().map(|s| s.to_string())))
    }

    /// Returns the inner HTML of the element.
    pub async fn inner_html(&self) -> XcelerateResult<String> {
        let res = self.call_js("function() { return this.innerHTML; }".to_string()).await?;
        Ok(res.result().value().and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }

    /// Dispatches a DOM event on the element (e.g. "blur", "change", "input", "focus",
    /// "keydown", "keyup", "keypress", "click", "mousedown", "mouseup").
    ///
    /// Picks the right event constructor based on `event_type` so that handlers reading
    /// event-specific properties actually see them: keyboard events need `KeyboardEvent`
    /// (for `key`/`code`/`keyCode`, since a plain `Event` leaves them empty), mouse events
    /// need `MouseEvent` (for `button`/`clientX`/`clientY`), everything else falls back to a
    /// plain bubbling, cancelable `Event`.
    ///
    /// `key` is only used for keyboard events: it sets `KeyboardEvent.key`/`.code` (e.g.
    /// "Enter", "a", "Escape") and derives `.keyCode`/`.which` from it for legacy handlers.
    /// Ignored for non-keyboard event types.
    pub async fn dispatch_event(&self, event_type: String, key: Option<String>) -> XcelerateResult<()> {
        const KEYBOARD_EVENTS: &[&str] = &["keydown", "keyup", "keypress"];
        const MOUSE_EVENTS: &[&str] = &[
            "click", "dblclick", "mousedown", "mouseup", "mouseover", "mouseout",
            "mousemove", "mouseenter", "mouseleave", "contextmenu",
        ];

        let event_type_json = serde_json::to_string(&event_type)?;
        let js = if KEYBOARD_EVENTS.contains(&event_type.as_str()) {
            let key_value = key.unwrap_or_default();
            let key_json = serde_json::to_string(&key_value)?;
            let key_code = key_code_for(&key_value);
            format!(
                "function() {{ this.dispatchEvent(new KeyboardEvent({}, {{ bubbles: true, cancelable: true, key: {}, code: {}, keyCode: {}, which: {} }})); }}",
                event_type_json, key_json, key_json, key_code, key_code
            )
        } else if MOUSE_EVENTS.contains(&event_type.as_str()) {
            format!(
                "function() {{ this.dispatchEvent(new MouseEvent({}, {{ bubbles: true, cancelable: true }})); }}",
                event_type_json
            )
        } else {
            format!(
                "function() {{ this.dispatchEvent(new Event({}, {{ bubbles: true, cancelable: true }})); }}",
                event_type_json
            )
        };
        self.call_js(js).await?;
        Ok(())
    }

    /// Executes an arbitrary JavaScript function body in the context of this element
    /// (`this` refers to the element) and returns the result serialized as a JSON string.
    ///
    /// This reuses the same `Runtime.callFunctionOn` mechanism as the built-in element
    /// methods (click, focus, etc.), so it does not open any new injection path beyond
    /// what stealth already accounts for.
    pub async fn evaluate(&self, script: String, timeout_ms: Option<u64>) -> XcelerateResult<String> {
        let params = js_protocol::runtime::CallFunctionOnParams::builder(script)
            .object_id(self.object_id.clone())
            .return_by_value(true)
            .await_promise(true)
            .build();
        let params_val = serde_json::to_value(&params)?;
        let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));

        let raw = tokio::time::timeout(
            timeout,
            self.page.client.execute_raw(
                js_protocol::runtime::CallFunctionOnParams::METHOD,
                Some(&self.page.session_id),
                params_val,
            ),
        )
        .await
        .map_err(|_| crate::error::XcelerateError::Timeout("evaluate timed out".into()))??;
        let res: js_protocol::runtime::CallFunctionOnReturns = serde_json::from_value(raw)?;

        if let Some(exception) = res.exception_details() {
            let message = exception.exception()
                .and_then(|e| e.description().map(|d| d.to_string()).or_else(|| e.value().map(|v| v.to_string())))
                .unwrap_or_else(|| exception.text().to_string());
            return Err(crate::error::XcelerateError::CdpResponseError { code: 0, message });
        }

        Ok(res.result().value().map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()))
    }
}

impl Element {
    /// Helper to call JS on this element.
    async fn call_js(&self, js: String) -> XcelerateResult<js_protocol::runtime::CallFunctionOnReturns<'static>> {
        let params = js_protocol::runtime::CallFunctionOnParams::builder(js)
            .object_id(self.object_id.clone())
            .build();
        let params_val = serde_json::to_value(&params)?;
        let raw = self.page.client.execute_raw(
            js_protocol::runtime::CallFunctionOnParams::METHOD,
            Some(&self.page.session_id),
            params_val,
        ).await?;
        Ok(serde_json::from_value(raw)?)
    }
}

/// Maps a `KeyboardEvent.key` value to its legacy `keyCode`/`which` numeric code,
/// for handlers that still rely on the deprecated numeric fields instead of `.key`/`.code`.
/// Falls back to the char code for single-character keys (e.g. "a" -> 65), and 0 for
/// anything unrecognized.
fn key_code_for(key: &str) -> u32 {
    match key {
        "Enter" => 13,
        "Tab" => 9,
        "Escape" | "Esc" => 27,
        "Backspace" => 8,
        "Delete" => 46,
        " " | "Spacebar" => 32,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Home" => 36,
        "End" => 35,
        "PageUp" => 33,
        "PageDown" => 34,
        "Shift" => 16,
        "Control" => 17,
        "Alt" => 18,
        _ => key
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase() as u32)
            .filter(|_| key.chars().count() == 1)
            .unwrap_or(0),
    }
}
