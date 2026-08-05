import { UniffiObjectBase } from "./runtime/objects.js";



export interface ComponentMetadata {
  namespace: string;
  packageName: string;
  cdylibName: string;
  nodeEngine: string;
  bundledPrebuilds: boolean;
  manualLoad: boolean;
}

export declare const componentMetadata: Readonly<ComponentMetadata>;

export { ffiMetadata } from "./xcelerate-ffi.js";


/**
 * Configuration for the Browser instance.
 */
export interface BrowserConfig {
  /**
   * Whether to run the browser in headless mode.
   */
  "headless": boolean;
  /**
   * Whether to apply stealth patches to the binary.
   */
  "stealth": boolean;
  /**
   * Whether to run the browser as a detached process.
   */
  "detached": boolean;
  /**
   * Optional path to the browser executable.
   */
  "executable_path": string | undefined;
  /**
   * Optional directory where downloaded files should be saved.
   */
  "download_path": string | undefined;
}

export declare class XcelerateError extends globalThis.Error {
  readonly tag: string;
  protected constructor(tag: string, message?: string);
}

export declare class XcelerateErrorWsError extends XcelerateError {
  readonly tag: "WsError";
  constructor(message?: string);
}

export declare class XcelerateErrorSerdeError extends XcelerateError {
  readonly tag: "SerdeError";
  constructor(message?: string);
}

export declare class XcelerateErrorCdpResponseError extends XcelerateError {
  readonly tag: "CdpResponseError";
  constructor(message?: string);
}

export declare class XcelerateErrorHttpError extends XcelerateError {
  readonly tag: "HttpError";
  constructor(message?: string);
}

export declare class XcelerateErrorNotFound extends XcelerateError {
  readonly tag: "NotFound";
  constructor(message?: string);
}

export declare class XcelerateErrorInternalError extends XcelerateError {
  readonly tag: "InternalError";
  constructor(message?: string);
}

export declare class XcelerateErrorTimeout extends XcelerateError {
  readonly tag: "Timeout";
  constructor(message?: string);
}

/**
 * Represents a browser instance (e.g., Chrome or Edge).
 */
export declare class Browser extends UniffiObjectBase {
  protected constructor();
  static launch(config: BrowserConfig): Promise<Browser>;
  /**
   * Closes the browser and kills the process.
   */
  close(): Promise<void>;
  new_page(url: string): Promise<Page>;
  /**
   * Returns the browser version information.
   */
  version(): Promise<string>;
}

/**
 * Represents an HTML element in the DOM.
 */
export declare class Element extends UniffiObjectBase {
  protected constructor();
  /**
   * Returns the value of a specific attribute.
   */
  attribute(name: string): Promise<string | undefined>;
  /**
   * Clicks the element.
   */
  click(): Promise<Element>;
  /**
   * Clicks the element using realistic mouse movement and CDP input events.
   */
  click_stealth(): Promise<Element>;
  /**
   * Dispatches a DOM event on the element (e.g. "blur", "change", "input", "focus",
   * "keydown", "keyup", "keypress", "click", "mousedown", "mouseup").
   *
   * Picks the right event constructor based on `event_type` so that handlers reading
   * event-specific properties actually see them: keyboard events need `KeyboardEvent`
   * (for `key`/`code`/`keyCode`, since a plain `Event` leaves them empty), mouse events
   * need `MouseEvent` (for `button`/`clientX`/`clientY`), everything else falls back to a
   * plain bubbling, cancelable `Event`.
   *
   * `key` is only used for keyboard events: it sets `KeyboardEvent.key`/`.code` (e.g.
   * "Enter", "a", "Escape") and derives `.keyCode`/`.which` from it for legacy handlers.
   * Ignored for non-keyboard event types.
   */
  dispatch_event(event_type: string, key: string | undefined): Promise<void>;
  /**
   * Executes an arbitrary JavaScript function body in the context of this element
   * (`this` refers to the element) and returns the result serialized as a JSON string.
   *
   * This reuses the same `Runtime.callFunctionOn` mechanism as the built-in element
   * methods (click, focus, etc.), so it does not open any new injection path beyond
   * what stealth already accounts for.
   */
  evaluate(script: string, timeout_ms: bigint | number | undefined): Promise<string>;
  /**
   * Focuses the element.
   */
  focus(): Promise<Element>;
  /**
   * Hovers over the element.
   */
  hover(): Promise<Element>;
  /**
   * Hovers over the element using realistic mouse movement.
   */
  hover_stealth(): Promise<Element>;
  /**
   * Returns the inner HTML of the element.
   */
  inner_html(): Promise<string>;
  /**
   * Returns the visible text of the element.
   */
  text(): Promise<string>;
  type_text(text: string): Promise<Element>;
}

export declare class Page extends UniffiObjectBase {
  protected constructor();
  /**
   * Evaluates a script on every new document.
   */
  add_script_to_evaluate_on_new_document(source: string): Promise<string>;
  /**
   * Moves the mouse to (x, y) and performs a click (down & up) with human-like delays.
   */
  click_mouse(x: number, y: number): Promise<Page>;
  /**
   * Returns the full HTML content of the page.
   */
  content(): Promise<string>;
  decode_base64(data: string): Uint8Array;
  /**
   * Executes an arbitrary JavaScript expression/script in the page's context and
   * returns its result serialized as a JSON string.
   *
   * Runs in the same `Runtime.evaluate` context used internally (title/content/etc.),
   * after the stealth payload has already been injected for this page, so it does not
   * bypass or race the anti-detection setup.
   *
   * `timeout_ms` bounds how long we wait for the CDP response. Without it a script that
   * blocks the page's JS event loop (e.g. `alert()`, or a promise that never settles since
   * `awaitPromise` is always on) hangs this call forever, since `Runtime.evaluate` simply
   * never replies until the loop is unblocked. Defaults to 30s when not provided.
   */
  evaluate(script: string, timeout_ms: bigint | number | undefined): Promise<string>;
  /**
   * Finds an element matching the CSS selector.
   */
  find_element(selector: string): Promise<Element>;
  /**
   * Finds all elements matching the CSS selector (equivalent to `document.querySelectorAll`).
   */
  find_elements(selector: string): Promise<Array<Element>>;
  go_back(): Promise<void>;
  /**
   * Triggers a mousePress event at the current mouse coordinates.
   */
  mouse_down(button: string): Promise<Page>;
  /**
   * Triggers a mouseReleased event at the current mouse coordinates.
   */
  mouse_up(button: string): Promise<Page>;
  /**
   * Moves the mouse cursor from the current position to the target (x, y) along a realistic Bezier curve.
   */
  move_mouse(x: number, y: number): Promise<Page>;
  /**
   * Navigates to a URL.
   */
  navigate(url: string): Promise<void>;
  pdf(): Promise<Uint8Array>;
  /**
   * Reloads the page.
   */
  reload(): Promise<void>;
  screenshot(): Promise<Uint8Array>;
  screenshot_full(): Promise<Uint8Array>;
  /**
   * Returns the page title.
   */
  title(): Promise<string>;
  /**
   * Waits for the page to finish loading.
   */
  wait_for_navigation(): Promise<void>;
  /**
   * Waits for an element matching the selector to appear in the DOM.
   */
  wait_for_selector(selector: string): Promise<Element>;
}