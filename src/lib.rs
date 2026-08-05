//! # xcelerate
//! 
//! `xcelerate` is a high-performance, lightweight Chrome DevTools Protocol (CDP) client.
//! It provides a fluent, chained API for browser automation, designed for speed and reliability.

pub mod error;
pub mod connection;
pub mod browser;
pub mod page;
pub mod element;
pub mod stealth;

pub use error::{XcelerateError, XcelerateResult};
pub use browser::{Browser, BrowserConfig};
pub use page::Page;
pub use element::Element;
pub use connection::{CdpClient, CdpHandler};

uniffi::setup_scaffolding!("xcelerate");

/// The core trait for defining CDP commands.
pub use connection::client::CdpCommand;

macro_rules! impl_cdp_command {
    ($params:ty, $returns:ty, $method:expr) => {
        impl CdpCommand for $params {
            type Response = $returns;
            const METHOD: &'static str = $method;
        }
    };
}

impl_cdp_command!(browser_protocol::page::NavigateParams<'static>, browser_protocol::page::NavigateReturns<'static>, "Page.navigate");
impl_cdp_command!(browser_protocol::page::EnableParams, serde_json::Value, "Page.enable");
impl_cdp_command!(browser_protocol::target::CreateTargetParams<'static>, browser_protocol::target::CreateTargetReturns<'static>, "Target.createTarget");
impl_cdp_command!(browser_protocol::target::AttachToTargetParams<'static>, browser_protocol::target::AttachToTargetReturns<'static>, "Target.attachToTarget");
impl_cdp_command!(js_protocol::runtime::EvaluateParams<'static>, js_protocol::runtime::EvaluateReturns<'static>, "Runtime.evaluate");
impl_cdp_command!(js_protocol::runtime::CallFunctionOnParams<'static>, js_protocol::runtime::CallFunctionOnReturns<'static>, "Runtime.callFunctionOn");
impl_cdp_command!(js_protocol::runtime::GetPropertiesParams<'static>, js_protocol::runtime::GetPropertiesReturns<'static>, "Runtime.getProperties");
impl_cdp_command!(js_protocol::runtime::ReleaseObjectParams<'static>, serde_json::Value, "Runtime.releaseObject");
impl_cdp_command!(browser_protocol::browser::GetVersionParams, browser_protocol::browser::GetVersionReturns<'static>, "Browser.getVersion");
impl_cdp_command!(browser_protocol::browser::CloseParams, serde_json::Value, "Browser.close");
impl_cdp_command!(browser_protocol::browser::SetDownloadBehaviorParams<'static>, serde_json::Value, "Browser.setDownloadBehavior");
impl_cdp_command!(browser_protocol::page::ReloadParams<'static>, serde_json::Value, "Page.reload");
impl_cdp_command!(browser_protocol::page::CaptureScreenshotParams<'static>, browser_protocol::page::CaptureScreenshotReturns<'static>, "Page.captureScreenshot");
impl_cdp_command!(browser_protocol::page::PrintToPDFParams<'static>, browser_protocol::page::PrintToPDFReturns<'static>, "Page.printToPDF");
impl_cdp_command!(browser_protocol::page::GetNavigationHistoryParams, browser_protocol::page::GetNavigationHistoryReturns<'static>, "Page.getNavigationHistory");
impl_cdp_command!(browser_protocol::page::NavigateToHistoryEntryParams, serde_json::Value, "Page.navigateToHistoryEntry");
impl_cdp_command!(browser_protocol::network::EnableParams, serde_json::Value, "Network.enable");
impl_cdp_command!(browser_protocol::target::GetTargetsParams<'static>, browser_protocol::target::GetTargetsReturns<'static>, "Target.getTargets");
impl_cdp_command!(browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams<'static>, browser_protocol::page::AddScriptToEvaluateOnNewDocumentReturns<'static>, "Page.addScriptToEvaluateOnNewDocument");
impl_cdp_command!(browser_protocol::page::GetLayoutMetricsParams, browser_protocol::page::GetLayoutMetricsReturns, "Page.getLayoutMetrics");
impl_cdp_command!(browser_protocol::emulation::SetDeviceMetricsOverrideParams<'static>, serde_json::Value, "Emulation.setDeviceMetricsOverride");
impl_cdp_command!(browser_protocol::emulation::ClearDeviceMetricsOverrideParams, serde_json::Value, "Emulation.clearDeviceMetricsOverride");
impl_cdp_command!(browser_protocol::input::DispatchKeyEventParams<'static>, serde_json::Value, "Input.dispatchKeyEvent");
impl_cdp_command!(browser_protocol::input::DispatchMouseEventParams<'static>, serde_json::Value, "Input.dispatchMouseEvent");
