# Plan: browser tabs on Linux

Hand this to an agent working on a Linux desktop. It describes where Linux
stands today, the target, and a phased path, each phase shippable and
verifiable on its own. Read `AGENTS.md` first; its rules apply throughout.

## Status: the `servo` feature (first round)

A build with the cargo feature `servo` on `sheprd-gpui` (off by default,
Linux only) shows pages inside the window on Wayland and on X11:

```sh
cargo build -p sheprd-gpui --features servo
```

- Servo 0.8 from crates.io draws each page offscreen, on one thread of its
  own for the whole app (`browser/engine`). The window paints the latest
  frame as a GPUI image (`browser/page/offscreen.rs`), so menus and toasts
  draw over a page and no child window exists. This replaces phases 2 and 3
  below for such a build; phase 1 still describes a default build.
- Servo runs inside the app's own process. A page that crashes the engine
  ends the whole app, with every window; wry's views on macOS and Windows
  are separate processes, and a crash there loses one page. When only the
  engine's thread ends, each tab says once that the engine stopped, and the
  app must be restarted to open pages again.
- The build script names the gates: `embedded_pages` (macOS, Windows, or
  Linux with the feature) means "a page exists", and `servo_pages` means
  Servo draws it. `any(target_os = "macos", windows)` stays where code
  needs wry itself, and for VS Code tabs, px0, and the code-server launcher
  (`browser::WRY_PAGES`): a Servo page has no clipboard and no input method
  yet, and none of the three was ever opened on Servo, so with the feature
  they behave as in a default Linux build.
- Chat pages (an HTML fence run inside a chat message) are plain local
  pages and use Servo with the feature. Nobody has opened one on Servo yet.
- The page rules are the same on both backends and live in
  `browser/policy.rs`: navigation only to web addresses and the preview
  scheme, new windows refused, no downloads, local files only through
  `herdr-preview` from the page's own folder, the bridge only on local
  pages. Servo always carries a `file:` handler, so the Servo path also
  refuses every `file:` request of a page. `policy::verdict` decides each
  request of a Servo page, in a fixed order that a test holds.
- One limit of the folder rule, the same as on wry: the preview scheme
  believes a request without a referrer in a tab that shows a local page.
  A web page that a local page embeds in a frame, and that hides its
  referrer, can therefore load files of that page's folder. Servo does not
  tell its host which document made a request.
- Works, checked in the native app: `browser open` for local files and web
  addresses, `--beside`, back and forward, close, resize, zoom, and
  `browser text`. The toolbar's button at the right opens a web address in
  the system browser and a local page's file with the system's handler for
  it. Reload is checked only by opening the edited file again.
- Not checked in the native app: keys typed into a page, `herdr.send`
  reaching the notes panel (the page's call returns true), annotation,
  favicons, find, the kit, Markdown pages, and chat pages.
- Servo keeps unfinished web features behind preferences that are off.
  The first one on is `dom_intersection_observer_enabled`: pages
  built with Lit make an `IntersectionObserver` while their modules load
  and stay empty without it (Authentik's sign-in page). No other Servo
  code reads the preference, and the binary does not grow. The cost is at
  run time: pages can run Servo's unfinished observer
  ([servo/servo#35767](https://github.com/servo/servo/issues/35767)),
  and Servo then runs the observer's steps with layout queries on each
  rendering update. Inferred from two TODO lines in Servo's observer, not
  reproduced: it does not apply the same-origin rules for `rootMargin`
  and `rootBounds`, so a framed cross-origin document that shares the top
  document's script thread may learn the size of the pane.
- Three more preferences are on, each proved with a test page in the
  native app (Servo 0.8.0, debug build), also with the element hidden,
  removed, and in a same-origin frame:
  - `dom_fontface_enabled`: `FontFace` and `document.fonts.add`. Without
    it a script that loads a font stops at "FontFace is not defined". A
    script can now start a font load. The request follows the same rules
    as one of an `@font-face` rule, and a font from another origin needs
    CORS. Checked: a `file:` address is refused, and the preview scheme
    gives a web page no font.
  - `layout_css_font_variations_enabled`: `font-variation-settings` and
    the weight axis of a variable font. Without it every weight of such
    a font is drawn alike. Checked with two well-formed fonts only.
  - `layout_css_backdrop_filter_enabled`: `backdrop-filter`. Without it
    the declaration is dropped and a frosted panel shows what is behind
    it unchanged. The pixels were checked for `invert(1)` only, in a
    scratch build that wrote the page's frame to a file.
- Known failure with `dom_fontface_enabled`, in a debug build: a page
  calls `document.fonts.clear()` and then removes the `<style>` that
  holds an `@font-face` rule. Servo's script thread for that page panics
  (`fontfaceset.rs:181`) and the tab stops, as described below for
  `IntersectionObserver`. The panic is behind `debug_assertions`; a
  release build should log a warning and go on. Not checked: a release
  build.
- Tested and left off, because they change nothing that a page shows:
  `layout_css_container_queries_enabled` and `layout_css_columns_enabled`.
  Servo then accepts `container-type` and `column-count`, but no
  `@container` rule applies and no column is laid out. A page that asks
  `CSS.supports` would get a wrong yes.
- Tested and left off, because a page can end the app with it:
  `dom_adoptedstylesheet_enabled`. A constructed style sheet that a
  shadow root adopts does work then. But a page that sets one entry of
  the list to a sheet that Servo refuses (`list[0] = style.sheet`,
  `NotAllowedError`) leaves Servo's own list one entry short. The next
  `push` or `pop` on the list panics in a function that cannot unwind
  (`documentorshadowroot.rs:444` and `:455`) and the whole app aborts; a
  new list assigned after it panics the page's script thread only.
  Reproduced with Servo 0.8.0 in a debug build. Without the preference,
  `adoptedStyleSheets` is not there. A component that tests for it
  first and else puts a `<style>` element in its shadow root is then
  drawn styled. That was checked with a component written by hand.
  Whether Lit does so was not checked: no copy of Lit was at hand.
- Servo is built with its cargo feature `subtlecrypto`, so a page in a
  secure context has `crypto.subtle`. Without it a script that needs it
  stops at "crypto.subtle is undefined"; sign-in pages use it. Tested
  with a page from `http://127.0.0.1` only: digests, HMAC, AES-GCM,
  PBKDF2, and ECDSA answer in a page, a frame, and a worker. A local
  file's page is not a secure context in Servo (its origin is `null`)
  and gets no `crypto.subtle`. To know about it:
  - The feature turns on Servo's whole SubtleCrypto set, not only what
    was tested: RSA, ML-KEM, ML-DSA, X-Wing, Ed448, X448, Argon2, KMAC
    and more.
  - Servo bounds the PBKDF2 iterations and the Argon2 memory only from
    below, and the work runs on the page's script thread. A page that
    chooses a large cost keeps that thread busy, so its tab stops
    answering, and a large Argon2 memory value is taken from the app's
    own process.
  - The build gains 34 crates, four of them pre-releases (`rsa`,
    `pkcs1`, `ed448-goldilocks`, `x448`).
  - One of them, `rsa 0.10.0-rc.19`, is under RUSTSEC-2023-0071, a
    timing side channel in RSA private-key operations. The advisory
    lists `rsa 0.10.0` and later as patched, so a release candidate is
    in its range. Servo pins that version, and a page's script reaches
    RSA signing and decryption. The key is the page's own. `deny.toml`
    has an `ignore` entry for the advisory with this reason; take it
    out when Servo pins a fixed `rsa`.
- Turn on another preference one at a time, with a page that proves it,
  in `browser/engine/host.rs`; never Servo's experimental group as a
  whole.
- Known failure of `IntersectionObserver`, reproduced with Servo 0.8.0
  in a debug build: a page observes an element inside a same-origin
  frame (`srcdoc`) that is drawn, and then gives the frame
  `display: none`. Servo's script thread for that page panics
  (`intersectionobserver.rs:970`). The tab
  stops: its title no longer changes and `browser text` ends with "did
  not finish loading in time". The app, its other tabs, and new tabs go
  on, the log gets `Sheprd panicked` with a crash report, and the tab
  closes as usual. Servo does not tell the host about it, so the tab
  shows no message. A frame that is hidden from the start, a cross-origin
  frame, and `unobserve` of an element that was never observed do not
  fail. Not checked: a release build.
- A page's console messages and uncaught script errors go to the app's
  log as `Page console` (`sheprd::browser::engine::host`): warnings and
  errors at INFO, the rest at DEBUG. Read them with `sheprd logs` first
  when a page stays empty. A message is cut at 1024 bytes, and each
  address in it loses its user information, its query, and its fragment.
  A path that starts with `/` and holds a `?`, as a page logs its own
  requests (`GET /api/users?token=...`), loses its query and fragment
  too. A path without a leading `/` (`api/x?token=1`), a bare query
  (`?code=1`), and a fragment of a path with no `?` (`/cb#token=1`) stay
  as logged.
- An answer without content gives a blank tab and no error. Servo 0.8.0
  draws its error page for a network failure (checked with a refused
  port). A server that answers a page's address with an error status and
  no body, or with a redirect status and no `Location`, gets an empty
  document, and Servo tells the host no status. A sign-in can end so, for
  example when the address that takes the code answers with an error
  status and no body. Not checked against a real service: which answer a
  real sign-in gave is not known. The tab then shows that address, and
  its title is the address's host, not the last page's title.
  `sheprd browser list` shows both; load the site's own address again to
  go on. The tab says so: a line above the page, "The site sent an empty
  page.", with a Reload button. The window learns it from the document
  with a script of its own, 0.7 s after the load and only while the tab
  is shown (Servo holds the timers of a hidden page), and never requests
  the address again, which may hold a code that works once. The engine
  runs that script before the page's own scripts, and it keeps the
  engine's accessors from then, so a page's scripts cannot choose the
  answer; a page that empties its own body does get the line. Only a
  web page gets the line, not a local file. A body with any element in
  it counts as content, so a page whose script draws into an element it
  already has gets no line. While the line shows the page is asked
  again every 2 s, and the line goes when the page has content or the
  tab loads again. A document that gives no answer three times in a row
  is not asked again.
  The line's button never sends a one-time code again unseen. At an
  address with a query or fragment, when the tab was asked for an
  address on that site in this run, the button opens that address
  (kept without its query), which starts the site's sign-in afresh.
  When the app knows no such address, as for a tab restored on the
  callback, the button is Reload and the line says that it sends the
  address again with any one-time code in it.
- The log budget belongs to the tab, not to the document. A tab may
  write 200 records at once and gets one back each second. Error lines
  have 50 more places of their own, with the same refill, so a page that
  fills its console cannot hide the uncaught error that follows.
  `console.error` uses the same places as an uncaught error: Servo
  reports both alike. The records of what the host refused a page (a
  navigation, a new window, a request) use the same budget. A record
  without a place is counted, and the next record that has one gives the
  count as `skipped`. A load the user asks for (reload, a new address,
  back, forward) gives the tab its whole budget again; a load the page
  starts gives nothing. A page that never stops can so write about
  2.6 KB each second for each tab, which fills the two log files of
  16 MiB in a few hours and replaces older records.
- Warning: console text is the page's text and can hold secrets, for
  example a token that a script prints. The log file is readable by its
  owner only (mode `0600`), and `sheprd diagnostics bundle` copies its
  last 500 records. The bundle cuts the query and the fragment of each
  address, but it cannot know every secret: read a bundle before you
  share it.
- `browser text` returns the text outside shadow roots only: for a page
  made of custom elements it can be almost empty while the page is drawn.
- The window reaches the engine through two bounded queues. Pointer moves
  and resizes keep only the latest per page; opening, closing, and showing
  a page wait in a queue of their own that input cannot fill.
- Quit: the quit handler tells the engine to stop and waits for it without
  blocking, inside GPUI's 200 ms for quit handlers. A release build needs
  about 130 ms with one page and 150 ms with five. After the event loop has
  returned and the windows are closed, the process waits at most 2 s more
  for a slower shutdown. Without the wait the process ended while Servo's
  threads still ran and crashed in SpiderMonkey's destructors.
- A `<select>` opens a list that the app draws over the page, at the
  control, in the app's theme (`browser/page/offscreen/select.rs`): Servo
  draws the control and leaves its list to the host. The list shows the
  options, the labels of groups, options that are off, and a mark at the
  current choice. A press chooses; so do the arrow keys, Home, End, Page
  Up, Page Down, typed letters, and Enter. Escape, a press outside the
  list, Tab, a wheel turn over the page, a new size or zoom, a
  navigation, another tab, and closing the tab close the list, and Servo
  is answered each time: for a `<select>` of one choice, that nothing
  changed. A `<select multiple>` gets the same list: a press or Space
  turns an option on or off; Enter, Tab, and a press outside keep the
  choices, and every other way of closing gives them up. The page hears
  no key the list took, nor its release; Tab also reaches the page,
  which moves its focus on.
  - The page's text is bounded before the window sees it: the first 2000
    rows, 200 characters each, without the characters that end a line or
    turn the direction of text. Options past those cannot be chosen from
    the list. Those of them that a `<select multiple>` has selected stay
    selected whatever is chosen in the list.
  - Servo reports where the control is in the CSS pixels of the control's
    own document. For a control in a frame that is not its place in the
    page, so the list opens under the press that opened it. A list that a
    frame's control opens from the keyboard is misplaced.
  - Servo tells the host about no scroll that a script does: such a list
    stays where it opened.
  - Every other control a page asks the host for is closed at once, so no
    page waits: `alert` returns, `confirm` answers false, `prompt` answers
    null, a color control keeps its color, and a file control gets no
    file. The log names each at DEBUG (`Closed a control a page asked
    for`), within the tab's log budget.
- Not there yet, each with an issue on the tracker: IME, clipboard, cursor
  shapes, the other controls the host must draw (color, file, `alert`,
  `confirm`, `prompt`), context menus, `target=_blank` links, note
  screenshots, and the release bundle (Servo adds about 113 MB). Servo
  draws some pages wrongly (inline SVG ignores page CSS, SVG `<text>`,
  `mask-image`): open such a page in the system browser with the toolbar
  button.
- No unit test starts Servo: under `cfg(test)` the engine's handle has no
  thread behind it.
- A default build compiles no Servo crate and behaves as described below.

### What sites keep between runs

A Servo tab keeps its cookies and site storage from one run of the app to
the next, so a restart does not sign every tab out, and a tab restored in
the middle of a sign-in can finish it. Servo 0.8.0 keeps this data in one
folder it is given (`Opts::config_dir`, as its own shell uses it).
The Walkr section's page uses this same folder:

```text
~/.local/state/herdr/gpui/browser-data/     ($XDG_STATE_HOME/herdr/gpui/...)
  cookie_jar.json     every cookie, also those without an expiry
  hsts_list.json      sites that asked for HTTPS only
  localstorage.json, webstorage/            localStorage
  clientstorage/                            IndexedDB
  auth_cache.json, cachestorage/            written by Servo, empty in this app
```

- Warning: the files are not encrypted. Cookies and stored values are
  plain text there, as in Servo's own shell. The folder has mode `0700`,
  so no other user can enter it; the app sets the mode at each start and
  sets the files of earlier runs to `0600`. Servo creates new files with
  the mode the process's umask gives (`0644` by default) until then.
- `auth_cache.json` is for names and passwords given to an HTTP sign-in
  prompt. The app shows no such prompt, so Servo has nothing to put
  there. The Cache API is kept in memory only.
- The app refuses a folder that is a symbolic link or belongs to another
  user. Servo then keeps nothing between runs, and the log says so.
- One Sheprd process has the folder at a time: its engine holds a lock on
  the folder until it ends. The Servo tabs of a second Sheprd started
  meanwhile keep nothing between runs, and the log says so.
- Servo writes the cookies and the HTTPS list only as it shuts down. A
  run that is killed, or whose shutdown takes longer than the quit wait,
  keeps the files of the run before it.
- Cookies without an expiry are kept too. Servo writes its whole jar.
- No HTTP cache is kept on disk.
- Other builds are not changed: on macOS and Windows the system's web view
  keeps its own data in its own store.

`sheprd browser clear-data` removes it. The command only opens a dialog in
the window that shows the caller's workspace; nothing goes until the user
confirms there.

- With no Servo page opened in this run, the folder is emptied at once.
- With pages opened, Servo has its storage files open, and Servo 0.8.0 has
  no call that drops IndexedDB or the HTTPS list from a running engine.
  The dialog and the command's answer say so before the user confirms.
  At the confirmation the cookie file and the HTTPS list are removed
  from the disk, and the engine drops its cookies, its cached answers,
  and the `localStorage` and `sessionStorage` of the sites it lists. A run
  that crashes or is killed after that cannot bring the old cookies back.
- The rest of the folder is emptied when the engine has shut down as the
  app quits, or at the next start if the run was killed. The cookie file
  Servo writes at that shutdown stays, because it holds only cookies set
  after the clearing. If the engine did not answer within 5 s, the window
  says so, and that cookie file goes too.
- Known limits until the restart: an open page keeps what it holds in its
  own memory, and IndexedDB data stays readable to its site.

## Where Linux stands (default build)

- `crates/sheprd-gpui/Cargo.toml` pulls `wry` only for macOS and Windows:
  "wry has no working GPUI child-view backend on Linux yet".
  `browser/page.rs` lays a native view out where its group draws.
- `browser::EMBEDDED` is `false` on Linux. `control::open_browser` then opens
  web addresses in the system browser and gives a local file a tab that
  shows its source (it refused local files with `ErrorCode::Unsupported`
  before the file view).
- About 160 `cfg(any(target_os = "macos", windows))` gates across 26 files
  leave out the parts that need a page: `browser/native.rs` (web views),
  `browser/preview.rs` (the `herdr-preview` scheme serving local files and
  the kit), `browser/annotate_view*` (notes panel and delivery), page
  presentation in `browser/view/present.rs`, reload in
  `browser/view/requests.rs`, and the VS Code tab's page in `browser/code.rs`.
  `grep -rn 'target_os = "macos", windows' crates/sheprd-gpui/src` lists them.
- macOS only: popups adopted as tabs (`browser/popup.rs`), WebKit snapshots
  for note screenshots (`browser/snapshot.rs`), page keys
  (`browser/page_keys.rs`), and pages beneath the window's drawing
  (`browser/stacking.rs`). Windows does without these; Linux can too.
- What already works on Linux and must keep working: tab strips, editor
  groups and splits, the control socket, `browser feedback`, notes sent from
  mockups, and the agent skill.
- `browser open` never places a tab on Linux: `open_browser` returns before
  any window is asked when `EMBEDDED` is false. Placement beside the
  caller's terminal (`--beside`, and the default for local files) therefore
  does not work yet, and comes with phase 2.

## Target

On a Linux desktop, `sheprd browser open` shows web addresses and local
files inside the window, beside terminals, with annotations, `herdr.send`,
the explainer kit, and reload, matching Windows' feature set. Where the
display server cannot embed a page, the app says so plainly and falls back
to the system browser, never to something that looks embedded but is not.

## Constraints that shape the design

- GPUI on Linux draws its own X11 or Wayland window; it does not use GTK.
- wry on Linux is WebKitGTK. `WebViewBuilder::build_as_child` works only on
  X11 (it reparents a GTK window into the X11 parent) and needs GTK
  initialized on the thread that runs it, with GTK events pumped. Wayland has
  no child-surface embedding through wry.
- Every page is untrusted. Keep the existing rules: navigation limited by
  `native::navigable`, no downloads, local files served only from the page's
  own folder with dot files refused, `preview::from_the_web` blocking web
  origins, the bridge only on local pages, and nothing reaching an agent
  until the user presses Send.
- No blocking I/O on the UI thread; bounded queues; no new runtime.

## Phase 1: local pages in the system browser (X11 and Wayland)

Smallest change that makes local files, the kit, and `herdr.send` work on
every Linux desktop, while pages still open outside the window.

1. Serve local pages over loopback HTTP from the app: bind
   `127.0.0.1:0` on a background thread, reuse `location::requested_file`,
   `preview::read`, and `kit::asset` for the bytes, and route each page by an
   unguessable per-page token in the path (`/p/<token>/...`) that maps to its
   folder. Refuse requests whose `Host` is not the bound address (DNS
   rebinding) and keep `from_the_web` semantics for `Origin`/`Referer`.
2. `control::open_browser` on Linux: for `Location::Local`, open
   `http://127.0.0.1:<port>/p/<token>/<entry>` in the system browser and
   answer `OpenedIn::SystemBrowser`. Tokens live in memory only and expire
   when the app quits.
3. `herdr.send` without `window.ipc`: serve the bridge as `/.herdr/bridge.js`
   for these pages and have it `POST /p/<token>/.herdr/send` with the same
   JSON. The app parses it with `annotate::Report::parse`, accepts only
   `Report::Sent`, and keeps it for the agent as page notes. With no
   in-window panel to confirm in, require a confirmation step the page cannot
   fake: a GPUI toast or dialog in the window ("The page sent a result:
   <summary>. Send to agent?"). Do not deliver straight from the HTTP request.
4. Tests: unit-test token routing, `Host` checks, path traversal, dot files,
   and the size limits, then the end-to-end flow with a loopback client in a
   test. Update the skill's "When it does not work" section for Linux.

## Phase 2: embedded pages on X11 and XWayland

1. Add a Linux dependency on `wry` with its GTK feature set, and give
   `browser/page.rs`'s `Page` a Linux branch with the same
   `show`/`hide`/bounds behavior. Keep the versions in root
   `[workspace.dependencies]`.
2. Initialize GTK once on the main thread at startup when the window is X11
   (`gtk::init()`), and pump it from the GPUI foreground loop with a bounded
   `while gtk::events_pending() { gtk::main_iteration_do(false) }` per tick.
   Measure the tick cost in release (`just run`); it must not starve terminal
   rendering.
3. Get the X11 window handle from GPUI's `HasWindowHandle` and use
   `build_as_child`, as `native::Pages::ensure` does on macOS and Windows.
   Position and size the child where `browser/page.rs`'s `PageElement` lays
   it out, and keep it in step on resize, split drags, scrolling sidebars,
   and HiDPI scale changes.
4. Widen the cfg gates from `any(macos, windows)` to include
   `all(target_os = "linux", feature-or-runtime X11)`. Prefer a runtime check
   (`EMBEDDED` becomes a function of the window's backend) so one binary
   serves X11 and Wayland, and route a Wayland window to phase 1.
5. Bring over, in order: page creation and presentation, placement beside
   the caller's terminal (local files by default, `--beside`, `--tab`),
   `herdr-preview`
   custom protocol (WebKitGTK supports custom URI schemes), the bridge as an
   initialization script, title and load events, reload, annotation picker
   and notes panel, and then note screenshots with
   `webkit_web_view_get_snapshot` if it is practical.
6. Overlays: an X11 child window sits above GPUI, so menus and toasts must
   hide the pages they reach, as on Windows (`present_browser`'s cover
   logic, which only Windows builds include today). Hidden pages show a
   blank, as on Windows.

## Phase 3: Wayland

Investigate before committing, and report findings rather than guessing:
WPE WebKit or WebKitGTK offscreen rendering into a GPUI image (input
forwarding and IME are the hard parts), or a CEF off-screen renderer. Until
one works, Wayland keeps phase 1 and says so in the UI and the skill.

## Verification for every phase

- `just format`, `just ci`, `just lint-linux`, and `just lint-windows`; a
  Linux-only gate is proven only on Linux.
- Default and all-feature builds, `--locked`.
- Native QA on both an X11 session and a Wayland session, recording which
  one each check ran on: open a URL and a local file, `--beside`, reload,
  annotate with an element, a text selection, and a region, Send to an agent
  in a pane, `herdr.send` from `sheprd browser example` and from a
  variants page, a theme switch picked up by `/.herdr/theme.css`, menus and
  toasts over a page, splitting and closing groups with pages, HiDPI, and
  quitting with pages open.
- Never describe headless tests as native verification. Screenshots of the
  real window go in the PR, light and dark, plus a narrow window.
