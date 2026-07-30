using uniffi.xcelerate;

Console.WriteLine("--- Xcelerate C# UniFFI Test App ---");

try
{
    Console.WriteLine("[TEST 1] Launching Browser...");
    var config = new BrowserConfig(headless: false, stealth: true, detached: false, executablePath: null);
    using var browser = await Browser.Launch(config);

    Console.WriteLine("[TEST 2] Creating New Page and Navigating to eSocial...");
    using var page = await browser.NewPage("https://login.esocial.gov.br/");

    Console.WriteLine("Waiting for page load...");
    await page.WaitForNavigation();

    string title = await page.Title();
    Console.WriteLine($"Page Title: {title}");

    Console.WriteLine("\n[TEST anchors] Listing all <a> elements (id, text, href, class) via FindElements...");
    var anchors = await page.FindElements("a");
    var anchorSummaries = new List<string>();
    foreach (var anchor in anchors)
    {
        string id = await anchor.Attribute("id") ?? "";
        string href = await anchor.Attribute("href") ?? "";
        string cls = await anchor.Attribute("class") ?? "";
        string text = await anchor.Evaluate("function() { return this.innerText; }", 5000);
        anchorSummaries.Add($"id='{id}' text='{text}' href='{href}' class='{cls}'");
    }
    Console.WriteLine($"Found {anchorSummaries.Count} <a> elements:");
    foreach (var summary in anchorSummaries)
    {
        Console.WriteLine($"  - {summary}");
    }

    Console.WriteLine("\n[TEST login-informativo-govbr] Reading #login-informativo-govbr text via Evaluate...");
    string? loginInformativo = await page.Evaluate(
        "document.querySelector('#login-informativo-govbr')?.innerText ?? null",
        5000);
    Console.WriteLine($"#login-informativo-govbr text: {loginInformativo ?? "(null / not found)"}");

    Console.WriteLine("\n[TEST dispatch_event] Dispatching a keydown('Enter') via script on <body>...");
    var docBody = await page.FindElement("body");
    var keydownTask = page.Evaluate(
        @"(function() {
            return new Promise((resolve) => {
                document.addEventListener('keydown', function handler(e) {
                    document.removeEventListener('keydown', handler);
                    resolve(JSON.stringify({ key: e.key, code: e.code, keyCode: e.keyCode }));
                }, { once: true });
                setTimeout(() => resolve('no keydown received'), 2000);
            });
        })()",
        5000);
    await Task.Delay(200); // give the listener time to attach before dispatching
    await docBody.DispatchEvent("keydown", "Enter");
    string keydownCapture = await keydownTask;
    Console.WriteLine($"keydown listener saw: {keydownCapture}");

    // Run last: alert() blocks the page's JS dialog until dismissed by a human,
    // so anything evaluated afterwards on this page would time out too.
    Console.WriteLine("\n[TEST alert] Evaluate with a short timeout against a blocking alert()...");
    try
    {
        string alertResult = await page.Evaluate("alert('teste')", 3000);
        Console.WriteLine($"alert() returned (unexpected, should have timed out): {alertResult}");
    }
    catch (Exception exTimeout)
    {
        Console.WriteLine($"[EXPECTED TIMEOUT] {exTimeout.GetType().Name}: {exTimeout.Message}");
    }

    Console.WriteLine("Closing Browser...");
    await browser.Close();

    Console.WriteLine("\n[SUCCESS] Test run finished.");
}
catch (Exception ex)
{
    Console.WriteLine($"[ERROR] Test run failed: {ex}");
}

Console.WriteLine("--- Test Session Finished ---");

/* Original Wikipedia smoke test kept for reference:
try
{
    Console.WriteLine("[TEST 3] Locating Elements and Standard Actions...");
    // Find the language select or search input
    var searchInput = await page.FindElement("input#searchInput");
    
    // Test Focus and TypeText
    await searchInput.Focus();
    Console.WriteLine("Typing search query...");
    await searchInput.TypeText("C# Programming Language");

    // Retrieve and verify the value of the input using an attribute check
    string? typedValue = await searchInput.Attribute("value");
    Console.WriteLine($"Typed Input Value: {typedValue}");

    Console.WriteLine("[TEST 4] Stealth Pointer Interactions...");
    // Find the submit button
    var searchButton = await page.FindElement("button[type=\"submit\"]");
    
    // Test HoverStealth and ClickStealth
    Console.WriteLine("Performing Stealth Hover...");
    await searchButton.HoverStealth();
    
    Console.WriteLine("Performing Stealth Click...");
    await searchButton.ClickStealth();

    Console.WriteLine("Waiting for navigation after submit...");
    await page.WaitForNavigation();

    // Check the updated title
    string newTitle = await page.Title();
    Console.WriteLine($"New Page Title: {newTitle}");

    Console.WriteLine("[TEST 5] Screenshot Capture...");
    byte[] screenshotBytes = await page.Screenshot();
    string screenshotPath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "wikipedia_search.png");
    await File.WriteAllBytesAsync(screenshotPath, screenshotBytes);
    Console.WriteLine($"Screenshot successfully saved to: {screenshotPath} (Size: {screenshotBytes.Length} bytes)");

    Console.WriteLine("[TEST 6] Closing Browser...");
    await browser.Close();

    Console.WriteLine("\n[SUCCESS] All standard browser automation tests executed successfully.");
}
catch (Exception ex)
{
    Console.WriteLine($"[ERROR] Test run failed: {ex.Message}");
    if (ex.InnerException != null)
    {
        Console.WriteLine($"[INNER ERROR] {ex.InnerException.Message}");
    }
    Console.WriteLine(ex.StackTrace);
}

Console.WriteLine("--- Test Session Finished ---");
*/

