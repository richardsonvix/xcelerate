# Xcelerate — Documentação de Arquitetura e API

> Documento de referência gerado a partir de análise do código-fonte em 2026-07-29.
> Objetivo: servir como referência técnica e ponto de restauração/entendimento caso o
> comportamento do projeto mude ou regrida no futuro.

## O que é

Xcelerate é uma biblioteca **Rust** (`Cargo.toml`) que implementa um cliente do
**Chrome DevTools Protocol (CDP)**, no mesmo espírito de Puppeteer/Playwright/Selenium,
porém mais enxuta e com foco específico em **evasão de detecção anti-bot**.

Não embarca um navegador próprio nem é um fork do Chromium. Em vez disso:

1. Localiza um **Chrome/Edge já instalado** na máquina do usuário.
2. Faz uma **cópia temporária** do executável e aplica um **patch binário** nela
   (o Chrome original do usuário nunca é modificado).
3. Sobe esse binário copiado como processo filho com `--remote-debugging-port`,
   conecta via **WebSocket** e fala **CDP** puro (mesmo protocolo usado por
   Puppeteer/Playwright por baixo dos panos).
4. Expõe a API Rust para outras linguagens via **UniFFI** (bindings gerados para
   C#, Python e JavaScript — ver `bindings/`).

Cargo.toml classifica o projeto como: *"A high-performance, lightweight Chrome
DevTools Protocol (CDP) client for Rust"*, keywords: `automation, cdp, chrome,
headless, stealth`.

## Estrutura do repositório

```
src/
  lib.rs                  ponto de entrada da crate / setup UniFFI
  browser.rs               Browser: lançamento do processo, config, new_page, close
  page.rs                  Page: navegação, captura, mouse "humano"
  element.rs                Element: interação com elementos DOM
  error.rs                  tipos de erro (XcelerateError)
  cdc_payload.js             payload JS injetado em toda página nova (stealth)
  connection/
    client.rs                CdpClient: envio de comandos CDP
    handler.rs                CdpHandler: loop de leitura do WebSocket / dispatch de eventos
    mod.rs
  stealth/
    mod.rs
    patcher.rs                BinaryPatcher: patch do executável do Chrome
    process.rs                 spawn detached, kill, registry global de PIDs
  bin/
    uniffi-bindgen.rs          gerador de bindings

bindings/
  csharp/xcelerate.cs          binding C# (gerado)
  python/xcelerate/            binding Python (gerado)
  javascript/                  binding JS/Node (gerado, via NAPI/FFI)

scripts/
  generate_csharp_bindings.py
  generate_python_bindings.py
  generate_javascript_bindings.py
  generate_all.py
  bump_version.py
```

Dependências principais (`Cargo.toml`): `uniffi` (bindings multi-linguagem),
`browser-protocol` + `js-protocol` (tipos do CDP), `tokio` (runtime async),
`tokio-tungstenite` (WebSocket), `reqwest` (HTTP, usado só para descobrir a URL
do WebSocket debugger), `regex`, `tempfile`, `libc`.

## Fluxo de lançamento do navegador (`src/browser.rs`)

`Browser::launch(config: BrowserConfig)`:

1. Resolve o executável: usa `config.executable_path` se fornecido, senão
   procura em caminhos padrão do SO (`find_chrome_executable`):
   - Windows: `Program Files\Google\Chrome\Application\chrome.exe`,
     `Program Files (x86)\...`, `Program Files (x86)\Microsoft\Edge\Application\msedge.exe`
   - macOS: `/Applications/Google Chrome.app/...`, `/Applications/Microsoft Edge.app/...`
   - Linux: `/usr/bin/google-chrome`, `chromium-browser`, `chromium`, `microsoft-edge-stable`
2. Cria um `user_data_dir` temporário (`tempfile::tempdir()`) — perfil limpo a
   cada execução.
3. Escolhe uma porta livre local para `--remote-debugging-port`.
4. Se `config.stealth == true`, chama `BinaryPatcher::patch_to_temp(&exe)` antes
   de spawnar (ver seção Stealth).
5. Monta os argumentos do processo (`setup_browser_args`):
   ```
   --remote-debugging-port=<porta>
   --remote-debugging-address=127.0.0.1
   --user-data-dir=<tmp>
   --no-first-run
   --no-default-browser-check
   --remote-allow-origins=*
   --no-startup-window
   ```
   Se `headless`, adiciona `--headless=new` e um `--user-agent` fixo de Chrome
   124 Windows (evita o UA "HeadlessChrome" default, que é um sinal clássico
   de detecção).
6. Spawna o processo: **detached** (`config.detached == true`, sobrevive ao
   processo pai via `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS` no Windows /
   `setsid()` no Unix) ou **atrelado** (`tokio::process::Child`, morre com o
   processo pai).
7. Faz polling em `http://127.0.0.1:<porta>/json/version` até obter o
   `webSocketDebuggerUrl` (timeout ~15s: 100 tentativas × 150ms).
8. Conecta via `tokio-tungstenite`, inicia `CdpHandler` em uma task assíncrona
   que faz o dispatch de respostas/eventos CDP.

`Browser::new_page(url)`:

1. Cria um target com `about:blank` (não navega direto para a URL final —
   isso garante que o payload stealth seja injetado **antes** de qualquer
   script da página real rodar).
2. Anexa (`Target.attachToTarget`, `flatten: true`) e obtém uma `session_id`.
3. Se stealth ativo, injeta `cdc_payload.js` via
   `Page.addScriptToEvaluateOnNewDocument` (roda em toda navegação futura
   dessa página, não só uma vez) e habilita o domínio `Page`.
4. Só então navega para a URL real (`page.navigate(url)`).

`Browser::version()` → `Browser.GetVersion` do CDP.
`Browser::close()` → tenta `Browser.Close` via CDP; se ainda vivo, mata o
processo filho.

## Camada de evasão anti-bot (stealth)

Duas camadas independentes, ambas controladas por `BrowserConfig.stealth`:

### 1. Patch binário (`src/stealth/patcher.rs`)

Ferramentas de automação clássicas (Selenium/ChromeDriver) injetam no
executável do Chrome um trecho de código nativo identificável, geralmente na
forma de uma função contendo a assinatura `window.cdc_...` (usada
historicamente pelo ChromeDriver para expor variáveis internas). Serviços
anti-bot (Cloudflare, DataDome, PerimeterX etc.) verificam a existência
dessas variáveis/assinaturas no binário ou no runtime.

`BinaryPatcher::patch_to_temp`:
1. Copia o executável original para `xcelerate_<nome_original>` **na mesma
   pasta** (preserva DLLs/dependências *side-by-side* necessárias no Windows).
2. Busca no binário copiado (bytes brutos) o padrão regex `\{window\.cdc.*?;\}`.
3. Se encontrado, sobrescreve esse trecho com
   `{console.log("xcelerate stealth active!")}`, preenchendo com espaços até
   igualar o tamanho original em bytes (para não deslocar offsets/estrutura
   do binário).
4. Se a cópia falhar por qualquer razão, retorna o caminho original sem
   patchar (fallback silencioso — não há erro fatal).
5. **Nota de fragilidade**: a regex depende de uma assinatura de bytes
   específica que pode não existir (ou mudar) em versões futuras/diferentes
   do Chrome. Se não encontrar o padrão, segue sem patchar, silenciosamente.

O arquivo original do usuário **nunca é alterado** — só a cópia temporária é.

### 2. Payload JavaScript injetado (`src/cdc_payload.js`)

Injetado via `Page.addScriptToEvaluateOnNewDocument`, portanto roda antes de
qualquer script da página, em toda navegação subsequente da mesma `Page`.

Técnicas aplicadas, em ordem:

| # | Técnica | Detalhe |
|---|---|---|
| 1 | `navigator.webdriver` | Remove a propriedade do protótipo e da instância |
| 2 | Limpeza de leaks `cdc_` | Varre `window`, `document`, `Navigator.prototype` procurando propriedades contendo `cdc_` ou `__$cdc_` e as deleta |
| 3 | `window.chrome` mock | Se ausente, cria um objeto completo (`app`, `runtime`, `loadTimes`, `csi`) igual ao exposto pelo Chrome real, ausente em muitos setups headless |
| 4 | `Function.prototype.toString` | Sobrescrito para retornar `"function X() { [native code] }"` em qualquer função que o script tenha modificado — impede detecção via introspecção (`fn.toString()` revelando código JS ao invés de nativo) |
| 5 | `navigator.permissions.query` | Corrige o caso `notifications` para bater com `Notification.permission` real (headless costuma divergir) |
| 6 | `navigator.plugins` | Se vazio, popula com entradas fake de PDF viewer (Chrome/Chromium/Edge/WebKit) — headless normalmente vem com 0 plugins, sinal de detecção |
| 7 | `navigator.languages` | Garante `['en-US', 'en']` se vazio |
| 8 | WebGL vendor/renderer | Sobrescreve `getParameter(37445)` (`UNMASKED_VENDOR_WEBGL`) → `"Google Inc. (NVIDIA)"` e `getParameter(37446)` (`UNMASKED_RENDERER_WEBGL`) → string de uma GPU NVIDIA real via ANGLE/D3D11 — contorna fingerprint de GPU/hardware |
| 9 | `navigator.deviceMemory` | Se `< 4`, força `8` |
| 10 | `navigator.hardwareConcurrency` | Se `< 2`, força `8` |

Todas as funções substituídas passam por `maskToString` para não vazar o
código-fonte real via `Function.prototype.toString`.

### 3. Interações "humanas" (`src/page.rs`, `src/element.rs`)

Movimento de mouse não é teleporte de coordenada — usa uma **curva de Bézier
cúbica** com:
- Dois pontos de controle deslocados lateralmente por um valor aleatório
  (`Lcg::range(-0.2, 0.2) * distance`), perpendicular à linha reta
  origem→destino.
- Easing customizado (`t < 0.5` cúbico de aceleração, senão desaceleração
  cúbica) para simular aceleração/desaceleração humana.
- Número de passos entre 12 e 60, proporcional à distância.
- Jitter aleatório (±0.4px) em cada passo intermediário.
- Delay aleatório entre passos (6–14ms).

`Lcg` (Linear Congruential Generator, `src/page.rs:8-35`) é um gerador
pseudoaleatório simples, semeado com o timestamp atual em nanossegundos —
usado em todo o código de stealth para variar timings e offsets.

`Element::click_stealth()` / `hover_stealth()`:
1. Faz `scrollIntoView({ block: 'center', inline: 'center' })` no elemento.
2. Lê seu `getBoundingClientRect()`.
3. Escolhe um ponto **aleatório dentro do elemento** (não sempre o centro):
   `x = rect.x + rect.width*0.15 + random(0, rect.width*0.7)` (mesma lógica
   para y) — evita cliques sempre no centro exato, outro sinal de bot.
4. Move o mouse até lá via curva Bézier (`move_mouse`) e, no caso de
   `click_stealth`, dispara `mouse_down` → delay aleatório (60–140ms) →
   `mouse_up`.

`Page::click_mouse(x, y)` (não-stealth de elemento, mas ainda "humano" no
timing): move o mouse, aguarda 50–130ms (latência de reação), `mouse_down`,
aguarda 60–140ms (tempo de clique), `mouse_up`.

Digitação (`Element::type_text`): foca o elemento e dispara um evento de
teclado CDP (`type: "char"`) por caractere, com 50ms de delay fixo entre
cada um — evita colar o texto instantaneamente (sinal óbvio de automação).

## Conexão / protocolo (`src/connection/`)

- `client.rs` (`CdpClient`): serializa comandos CDP, associa `id` de
  requisição a uma `oneshot` channel, envia pelo WebSocket e aguarda a
  resposta correspondente. Suporta execução com ou sem `sessionId` (necessário
  para comandos direcionados a um target/page específico via `flatten`
  sessions).
- `handler.rs` (`CdpHandler`): loop que lê continuamente do WebSocket,
  decodifica JSON, e roteia para o `oneshot` correto (respostas) ou para um
  canal de eventos (mensagens CDP assíncronas tipo `Page.loadEventFired`).

## API pública (superfície UniFFI → C#/Python/JS)

A API é definida em Rust com atributos `#[uniffi::export]` /
`#[derive(uniffi::Object)]` / `#[derive(uniffi::Record)]`, e o UniFFI gera
bindings idiomáticos 1:1 para cada linguagem alvo. Exemplos abaixo em C#
(namespace `uniffi.xcelerate`), mas os mesmos métodos existem em Python/JS
com convenção de nome adaptada (`snake_case` em Python/Rust,
`camelCase`/`PascalCase` em C#/JS conforme o gerador).

### `BrowserConfig` (record/struct)

| Campo | Tipo | Default | Descrição |
|---|---|---|---|
| `headless` | bool | `true` | roda sem janela visível |
| `stealth` | bool | `true` | ativa patch binário + payload JS + timings humanos |
| `detached` | bool | `true` | processo sobrevive independente do processo host |
| `executablePath` | string? | `null` | caminho customizado do Chrome/Edge |

### `Browser`

| Método | Assinatura | Descrição |
|---|---|---|
| `Launch` (construtor estático) | `Task<Browser> Launch(BrowserConfig)` | encontra/patcha/spawna o navegador e conecta via CDP |
| `NewPage` | `Task<Page> NewPage(string url)` | abre novo target, injeta stealth, navega |
| `Version` | `Task<string> Version()` | string de versão + protocolo CDP |
| `Close` | `Task Close()` | fecha via CDP e garante kill do processo |

### `Page`

| Método | Assinatura | Descrição |
|---|---|---|
| `Navigate` | `Task Navigate(string url)` | navega a URL |
| `Reload` | `Task Reload()` | recarrega |
| `GoBack` | `Task GoBack()` | `window.history.back()` |
| `WaitForNavigation` | `Task WaitForNavigation()` | polling em `document.readyState === 'complete'`, timeout 30s |
| `FindElement` | `Task<Element> FindElement(string selector)` | `document.querySelector`, erro se não achar |
| `FindElements` | `Task<List<Element>> FindElements(string selector)` | `document.querySelectorAll`, lista vazia se nada casar |
| `WaitForSelector` | `Task<Element> WaitForSelector(string selector)` | polling a cada 250ms por até 30s |
| `Evaluate` | `Task<string> Evaluate(string script)` | executa expressão/script JS avulso via `Runtime.evaluate` (`returnByValue` + `awaitPromise`), retorna o resultado serializado em JSON (string `"null"` se sem retorno); lança `CdpResponseException` se o script lançar exceção |
| `Title` | `Task<string> Title()` | `document.title` |
| `Content` | `Task<string> Content()` | `document.documentElement.outerHTML` |
| `Screenshot` | `Task<byte[]> Screenshot()` | screenshot do viewport (PNG) |
| `ScreenshotFull` | `Task<byte[]> ScreenshotFull()` | screenshot da página inteira (ajusta `Emulation.setDeviceMetricsOverride` temporariamente) |
| `Pdf` | `Task<byte[]> Pdf()` | exporta a página como PDF |
| `AddScriptToEvaluateOnNewDocument` | `Task<string> AddScriptToEvaluateOnNewDocument(string source)` | injeta script customizado em toda navegação futura |
| `MoveMouse` | `Task<Page> MoveMouse(double x, double y)` | move o cursor via curva Bézier "humana" |
| `MouseDown` / `MouseUp` | `Task<Page> MouseDown(string button)` / `MouseUp(string button)` | `button`: `"left"`, `"right"`, `"middle"`, `"back"`, `"forward"` |
| `ClickMouse` | `Task<Page> ClickMouse(double x, double y)` | move + down + delay + up em coordenadas absolutas |

Todos os métodos que retornam `Page`/`Element` fazem *method chaining*
(retornam `self`).

### `Element`

| Método | Assinatura | Descrição |
|---|---|---|
| `Click` | `Task<Element> Click()` | `element.click()` via JS puro (instantâneo, não passa por CDP Input) |
| `ClickStealth` | `Task<Element> ClickStealth()` | scroll até o centro, calcula ponto aleatório dentro do elemento, clique via mouse simulado (Bézier + delays) |
| `Hover` | `Task<Element> Hover()` | dispara `MouseEvent('mouseover')` via JS |
| `HoverStealth` | `Task<Element> HoverStealth()` | move o mouse real (CDP Input) até ponto aleatório dentro do elemento |
| `Focus` | `Task<Element> Focus()` | `element.focus()` |
| `TypeText` | `Task<Element> TypeText(string text)` | foca e digita char a char com 50ms de delay via eventos de teclado CDP |
| `Text` | `Task<string> Text()` | `innerText` |
| `Attribute` | `Task<string?> Attribute(string name)` | `getAttribute(name)` |
| `InnerHtml` | `Task<string> InnerHtml()` | `innerHTML` |
| `DispatchEvent` | `Task DispatchEvent(string eventType)` | dispara `new Event(eventType, { bubbles: true, cancelable: true })` no elemento (ex.: `"blur"`, `"change"`, `"input"`, `"focus"`) |
| `Evaluate` | `Task<string> Evaluate(string script)` | executa um corpo de função JS com `this` = o elemento (via `Runtime.callFunctionOn`, `returnByValue` + `awaitPromise`), retorna o resultado serializado em JSON |

### Erros (`XcelerateException`, hierarquia)

`WsException`, `SerdeException`, `CdpResponseException`, `HttpException`,
`NotFound`, `InternalException` — todos herdam de `XcelerateException` →
`UniffiException` → `System.Exception` em C#.

## O que a API **não** oferece (limitações conhecidas)

- Sem XPath (só CSS selector via `querySelector`).
- Sem interceptação/mock de rede (fetch/XHR, request/response hooks).
- Sem gerenciamento explícito de cookies/localStorage/sessionStorage via API
  dedicada.
- Sem múltiplas abas/páginas geridas centralmente pelo `Browser` (cada
  `NewPage` é independente; não há `Browser.Pages()` listando as existentes).
- `FindElements`/`GetProperties` não garantem ordem estrita para NodeLists
  muito grandes (na prática o V8 retorna em ordem de índice, mas isso não é
  uma garantia formal do protocolo CDP).
- O patch binário depende de uma assinatura de bytes (`{window.cdc.*?;}`)
  que pode deixar de existir em versões futuras do Chrome; se isso ocorrer,
  o patch é pulado silenciosamente (sem erro, sem aviso).

## Exemplo mínimo de uso em C#

```csharp
using uniffi.xcelerate;

var config = new BrowserConfig(headless: false, stealth: true, detached: false, executablePath: null);
using var browser = await Browser.Launch(config);

using var page = await browser.NewPage("https://www.wikipedia.org/");
await page.WaitForNavigation();

var searchInput = await page.FindElement("input#searchInput");
await searchInput.Focus();
await searchInput.TypeText("C# Programming Language");

var searchButton = await page.FindElement("button[type=\"submit\"]");
await searchButton.HoverStealth();
await searchButton.ClickStealth();
await page.WaitForNavigation();

byte[] screenshot = await page.Screenshot();
await File.WriteAllBytesAsync("result.png", screenshot);

await browser.Close();
```

(Fonte: `bindings/csharp/Xcelerate.TestApp/Program.cs`, exemplo real do
próprio repositório.)
