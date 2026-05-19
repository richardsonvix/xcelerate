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
            let mut params = browser_protocol::input::DispatchKeyEventParams {
                type_: "char".into(),
                ..Default::default()
            };
            params.text = Some(c.to_string());
            params.unmodifiedText = Some(c.to_string());

            self.page.client.execute_with_session(
                Some(&self.page.session_id),
                params
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
        let val_str = res.result.value
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
        let val_str = res.result.value
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
        Ok(res.result.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }

    /// Returns the value of a specific attribute.
    pub async fn attribute(&self, name: String) -> XcelerateResult<Option<String>> {
        let js = format!("function() {{ return this.getAttribute('{}'); }}", name);
        let res = self.call_js(js).await?;
        Ok(res.result.value.and_then(|v| v.as_str().map(|s| s.to_string())))
    }

    /// Returns the inner HTML of the element.
    pub async fn inner_html(&self) -> XcelerateResult<String> {
        let res = self.call_js("function() { return this.innerHTML; }".to_string()).await?;
        Ok(res.result.value.and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_default())
    }
}

impl Element {
    /// Helper to call JS on this element.
    async fn call_js(&self, js: String) -> XcelerateResult<js_protocol::runtime::CallFunctionOnReturns> {
        self.page.client.execute_with_session(
            Some(&self.page.session_id),
            js_protocol::runtime::CallFunctionOnParams {
                functionDeclaration: js,
                objectId: Some(self.object_id.clone()),
                ..Default::default()
            }
        ).await
    }
}
