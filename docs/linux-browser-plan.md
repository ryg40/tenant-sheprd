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

- Servo draws each page offscreen, on one thread of its own for the whole
  app (`browser/engine`). The window paints the latest frame as a GPUI
  image (`browser/page/offscreen.rs`), so menus and toasts draw over a page
  and no child window exists. This replaces phases 2 and 3 below for such
  a build; phase 1 still describes a default build.
- Servo is an unreleased commit of its main branch, not a release:
  `1879c93b` (2026-11-19), pinned by git in the root `Cargo.toml`, with the
  `stylo` crates that commit takes from git too. Main still calls itself
  0.8.0. It has fixes that 0.8.0 lacks: no panic in `IntersectionObserver`
  for a frame without a padding box, `rsa` 0.10.0 in place of a release
  candidate, and the box of a `<select>` placed with frames and zoom. The
  plan is to return to crates.io with Servo 0.9.0: set
  `version = "=0.9.0"` in the root `Cargo.toml`, and remove the two
  `allow-git` entries of `deny.toml`. Checks below that name Servo 0.8.0
  were made with that release and not repeated on main, unless they say so.
- Servo runs inside the app's own process. A page that crashes the engine
  ends the whole app, with every window; wry's views on macOS and Windows
  are separate processes, and a crash there loses one page. When only the
  engine's thread ends, each tab says once that the engine stopped, and the
  app must be restarted to open pages again. When only the script thread
  of a page ends, its tab says that the page stopped and offers a reload
  (below).
- The build script names the gates: `embedded_pages` (macOS, Windows, or
  Linux with the feature) means "a page exists", and `servo_pages` means
  Servo draws it. `any(target_os = "macos", windows)` stays where code
  needs wry itself, and for VS Code tabs, px0, and the code-server launcher
  (`browser::WRY_PAGES`): a Servo page has no input method yet, and none
  of the three was ever opened on Servo, so with the feature they behave
  as in a default Linux build.
- Chat pages (an HTML fence run inside a chat message) are plain local
  pages and use Servo with the feature. Nobody has opened one on Servo yet.
- The page rules are the same on both backends and live in
  `browser/policy.rs`: navigation only to web addresses and the preview
  scheme, new windows only as new tabs, no downloads, local files only through
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
  build. The same check is still in Servo main at the pin
  (`fontfaceset.rs:275`), and a debug build of the app with the pin
  stops such a page the same way.
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
  - The build gains 34 crates (counted with Servo 0.8.0), two of them
    pre-releases at the pin (`ed448-goldilocks`, `x448`).
  - Servo 0.8.0 pinned `rsa 0.10.0-rc.19`, which is under
    RUSTSEC-2023-0071, a timing side channel in RSA private-key
    operations. Servo main at the pin takes `rsa 0.10.0`, the first
    version the advisory lists as patched, so `deny.toml` no longer
    ignores the advisory. A page's script still reaches RSA signing and
    decryption, with a key that is the page's own.
- Turn on another preference one at a time, with a page that proves it,
  in `browser/engine/host.rs`; never Servo's experimental group as a
  whole.
- Fixed with Servo main at the pin: a failure of `IntersectionObserver`,
  reproduced with Servo 0.8.0 in a debug build. A page observes an
  element inside a same-origin frame (`srcdoc`) that is drawn, and then
  gives the frame `display: none`. Servo 0.8.0's script thread for that
  page panics (`intersectionobserver.rs:970`), and the page stops, as
  the next point describes. Servo main returns no box for
  such a frame in place of the panic (`4071c845`). Checked in the app
  with a debug build of the pin: the same page hides and shows the frame
  and its parent, removes the frame, and runs to its end; the observer
  reports each change, and the log has no panic.
- A page stops when its script thread panics, as the
  `IntersectionObserver` failure above did with Servo 0.8.0
  (`intersectionobserver.rs:970`). The tab then shows
  "This page stopped working." in place of the page, with a Reload button.
  Reload, the toolbar's reload, and `sheprd browser reload` open the page
  again at the address the tab shows, in a new view of the window that
  had it, and choose no other tab; showing the tab again does the same.
  When the engine refuses the new page, as with a full queue, the tab
  shows that the page could not be made, and `sheprd browser reload`
  exits 1 and counts only the tabs that did reload.
  Back and Forward are off on such a tab: the new view gets the address,
  not the old view's history. The app, its other tabs, and new tabs go
  on. The panic is not a crash of the app: no crash report and no
  `Sheprd panicked`, only a WARN record with the thread's name
  (`Script#<n>`) and the place of the panic, without its message. A
  frame that is hidden from the start, a cross-origin frame, and
  `unobserve` of an element that was never observed do not fail. Checked
  with Servo 0.8.0 in a debug build. Checked again with Servo main at the
  pin in a debug build, with the `document.fonts.clear()` page above as
  the panic: the tab showed the line and Reload, Reload brought the page
  back, and a second tab of the same site was not told and went on.
- How the app learns that a page stopped: Servo main at the pin, as
  0.8.0 did, calls `notify_crashed` only when the `log` logger of
  `Servo::setup_logging` passes it the panic, which the app does not
  install, or when a later message to the ended thread fails. The crash
  hook contains the panics of threads named `Script#`
  (`sheprd_diagnostics::crash::contain`). Once
  such a thread has ended, it sets a flag and wakes the engine, which
  then asks every page a question. Servo answers with the same internal
  error when the thread of the page's current document is gone and when
  the page has no current document yet. So the app tells a tab only when
  its page had a first history entry and no running load when it was
  asked, and started no load and no Back or Forward step before it
  answered with that error; Servo's source says that such an error comes
  from the thread being gone. A healthy answer ends the asking. Only a
  page whose answer is not known (that error across a load, or no
  document yet) is asked again, when a load or a Back or Forward step of
  it completes, at most three times for one end. A page whose thread
  ends while it loads is therefore not told and keeps its last picture.
  A page that is busy (a script that works for 30 s was checked) answers
  later and is not reported. A question that waits on a busy page is
  lost if that page's own thread ends before it reads it, so each end
  asks such a page once more, with at most four questions waiting per
  page: a question to a thread that is gone fails at once. A busy page
  through whose busy time four other script threads ended is not told of
  its own end. The tab's report waits with its page until the window
  takes it, so a full queue of reports does not lose it. Inferred from
  Servo's source, not checked: a script thread serves one tab, as each
  tab's view is a browsing context group of its own, with its top
  document and the frames of the same site; a thread that ran only a
  frame of another site leaves that frame blank, and the tab is not
  told.
- What remains of a page's panic: one that cannot unwind from its start,
  such as a failed check of an unsafe precondition, on a script thread
  ends the app with only the WARN record and no crash report
  (`PanicHookInfo::can_unwind` is not stable). The WARN record is best
  effort: a full log queue drops it, as any record, and the flush after
  it cannot bring a dropped record back. A build that aborts on
  panic contains nothing: every panic there gets the whole report.
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
- The pointer has the shape the page asks for: the hand over a link, the
  I-beam over text, the resize arrows, `not-allowed`, `grab`, and the
  others GPUI can draw (`browser/engine/cursor.rs`). A shape that GPUI
  cannot draw (`wait`, `help`, `move`, `zoom-in`) becomes the nearest one
  or the arrow, and `cursor: none` shows the arrow: a page cannot hide the
  pointer. The engine keeps only the latest shape of each page and wakes
  the window when it changes, so a shape that changes under a resting
  pointer shows at once. The window shows the arrow from the start of a
  new document, of a document Servo restores from its history, and of
  each other change of the page's address (Servo's `notify_url_changed`,
  also for `pushState` and a fragment), and from the moment a page is
  hidden or shown again. So a page that changes its address by script
  can show the arrow until the next pointer move, when Servo reports no
  shape before it. The arrow stays until
  Servo reports a shape, or reports as handled
  (`notify_input_event_handled`) a pointer move that the engine sent
  after that moment: the first one, or one of the 16 latest, also the
  place the window sends ahead of a press, a key or a wheel turn. A page
  that is closed, or whose engine ended, shows the arrow. A new
  document has no shape and no pointer place of its own yet, so the
  first move that Servo tests in it sets Servo's shape before Servo
  reports the move as handled. The window shows that shape when Servo
  reports it, or when the handled move is one the engine still keeps,
  also when the shape equals the one Servo had before and Servo
  reported nothing. A move that 16 later ones have replaced reads
  nothing; a later move then does.
  Not guaranteed for a document that Servo main at the pin, as 0.8.0,
  restores from its history: that document sends no shape that equals its
  own last one, while Servo keeps one shape for all documents of the page.
  So a handled move can read the shape of the document shown before, and
  the window shows it until the shape under the pointer changes. The host
  cannot see this case. A move that Servo handled without a test of the
  pointer's place (its document went away, or the document's test found
  nothing) also reads Servo's last shape. Under a resting pointer, the
  page's shape can wait until the pointer moves.
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
  - Servo main asks for the list as the button goes down on the control;
    0.8.0 asked at the click. A press on the control while its list is
    open closes the list, and the page does not hear that press, so the
    list does not open again.
  - Known failure with the pinned Servo, seen in the app: the first
    press on a `<select>` that does not have the keyboard opens no list,
    and the second press opens it. Servo asks for the list as the
    button goes down on the control (`htmlselectelement.rs:1020` at the
    pin). Its default action for the press then gives the control the
    keyboard, and the control's handler of focus events takes the
    request back at once: it hides the list on every focus event but
    `blur` (`htmlselectelement.rs:1007`). Servo then drops the app's
    answer for that list (`document_embedder_controls.rs:217`), so the
    app cannot keep it open. The second press does not move the
    keyboard, so its list stays. Only a press opens a list at the pin:
    `<select>` has no `showPicker()`, and no key opens one (read in
    Servo's code, not checked in the app). The owner accepted this on
    2026-11-20 and kept the pin. It goes away with a Servo that fixes
    this focus handling: check it again at each change of the pin.
  - Servo reports where the control is in the page's device pixels, with
    the page's zoom, its scroll, and the frames around the control
    applied (Servo main; 0.8.0 reported CSS pixels of the control's own
    document, wrong inside a frame). The list opens there: checked in the
    app for a control in a same-origin frame and at a zoom of 150 %. That
    box is right only when Servo knows each frame between the control
    and the page. Servo stops at the first frame it does not know and
    still applies the frames it passed (`webview_renderer.rs:1253` at
    the pin), so such a box can have a size and lie at a wrong place.
    Servo does not tell the app when that happened, and a box far from
    the press is also what a page gives that moves the control during
    the press. So the app uses every box that has a size: for a frame
    that Servo does not know, the list can open at a wrong place (read
    in Servo's code; such a frame was not made in the app). Only a box
    with no size, which Servo gives when it cannot place the control,
    opens the list under the press instead.
  - Servo tells the host about no scroll that a script does: such a list
    stays where it opened.
  - Every other control a page asks the host for is closed at once, so no
    page waits: `alert` returns, `confirm` answers false, `prompt` answers
    null, a color control keeps its color, and a file control gets no
    file. The log names each at DEBUG (`Closed a control a page asked
    for`), within the tab's log budget.
- Copy, cut, and paste use the app's clipboard, text only
  (`browser/engine/clipboard.rs`): Ctrl+C or Ctrl+X in a page puts the
  selection where other apps and the terminal panes find it, and Ctrl+V
  in a field takes text copied anywhere. These three keys are all: Servo
  main at the pin, as 0.8.0, pastes for Ctrl+V alone, so Shift+Insert,
  Ctrl+Shift+V, and Ctrl+Insert reach the page as keys but perform no
  built-in clipboard action, and the keyboard layout must give the key the
  letter V, C, or X. A copied file or image pastes nothing. The host holds
  the rule itself, whatever Servo's preferences say:
  - Ctrl+V grants the page one read, and Ctrl+C or Ctrl+X grants it
    writes and clears, each for 5 s after the window sent the key: the
    window puts the time and the page's grant generation on the key as
    it sends it. Every other read gets nothing, and every other write
    changes nothing. Servo's `document.execCommand` has no `copy`,
    `cut`, or `paste`, and `navigator.clipboard` exists only with
    `dom_async_clipboard_enabled`, which is off here.
  - Every grant of a page ends when the page hides or loses the
    keyboard, when its window stops being the active window, when the
    app sends it a load, a reload, or a step back or forward (before
    Servo starts it), when Servo reports the start of a load, when the
    page shows another address, and when it closes. Each of these
    advances the page's generation. A key that the window sent before
    one of these ends still goes to the page as a key, but with no
    grant, also when the end reaches the engine first. Servo reports no
    start for a reload that the app asks for, nor for a step to a page it
    kept, so the host does not wait for one.
  - The engine checks a grant when the key reaches it, when the page
    asks, when it hands a write to the window, and when the window's
    answer to a read arrives. A read after the 5 s, or after its grant
    ended, gets a failure, also when the window has read the clipboard
    for it already: the engine drops that answer. A write after the
    5 s, or after its grant ended, does not reach the window. The window
    checks the grant, and that it is the active window, just before it
    touches the system clipboard.
  - One copy or cut is one change of the clipboard, and it reaches the
    window ahead of every later read of the same page, which the window
    serves after the write: Ctrl+V right after Ctrl+X pastes the cut
    text. The text of a selection goes to the window as soon as Servo
    makes it. A copy event that the page fills waits in the engine:
    Servo first clears the clipboard, then sends each text of the event,
    and gives no sign of the last one. That copy ends at the first of
    three points: the next clear, which starts another copy; the page's
    next read; or Servo's report that it handled a copy or cut key, which
    Servo sends right after the keys it handled together. The wait is
    that of Servo's report: Servo queues it after the page's script has
    handled every key of that group, so a slow group waits as long.
    At its end the copy's last text goes to the window, or a clear when
    it has no text; when one of its texts is over 1 MiB, nothing of the
    copy does, so the clipboard keeps what it had and a later copy still
    counts. The limit counts each text by itself, as only the last one
    reaches the clipboard. For a clear the window puts an empty text on
    the clipboard, as GPUI cannot empty it.
    Three limits remain. While more than one copy or cut key waits in
    Servo (two presses at once, or a held key in a page that is slow),
    Servo's messages do not tell the second text of one filled copy from
    the selection that the next key copied, and the engine takes it for
    the next copy: of a filled copy with a short text and a text over
    1 MiB, the short one can then be on the clipboard, and a copy that
    only clears is lost when the next key copies a selection over 1 MiB.
    And a filled copy waits for Servo's report: when a later key of the
    same group stops the page's script for good, the copy never reaches
    the clipboard. And while two keys wait in Servo, the first one's
    copy only clears, and the second copies a selection, that selection
    waits as a filled copy does, and is lost when Servo's report comes
    after the 5 s.
  - One copy or paste carries at most 1 MiB of text: a longer copy
    changes nothing, and a paste of longer text gets none. At most 4
    reads of a page wait for its window, which answers them when the
    engine wakes it, also while it draws nothing; a read past those 4 is
    refused at once. Servo's paste holds the page's script thread until
    it has its answer, so the engine fails an unanswered read itself:
    its loop wakes at the read's deadline, 5 s after the key, and on
    that turn it answers the read a failure and takes it out of what the
    window finds. A window that does not draw, a full command queue, a
    lost close, and a tab that moved to another window all end in that
    failure. The window reads the clipboard once for a read; when the
    engine's queue does not take the answer, the window drops it and
    does not read again.
  - Limits that stay:
    - A page that changes its address by script (also a fragment or
      `pushState`) while a copy or paste is under way can lose that
      copy or paste, because Servo reports every address change the
      same way.
    - A popup or a second window of the app that takes the activation
      right after a copy can cancel that copy.
    - The window checks the grant just before it touches the system
      clipboard. A revocation that arrives between that check and the
      access cannot be undone for a write. The write is still the text
      that the user's own copy key allowed for that document.
  - Servo names only the page (`WebView`) of a request, not its frame or
    its document. With `dom_async_clipboard_enabled` on, a script in any
    frame of the page could take the read that Ctrl+V granted, or write
    within the 5 s after Ctrl+C. So the preference must stay off
    (`preferences()` in `engine/host.rs`).
  - Neither thread waits for the other, but GPUI's `read_from_clipboard`
    waits on the UI thread, as every paste in the app does. On X11 it
    waits about 4 s for the app that holds the clipboard. On Wayland it
    waits up to 4 s for each piece of the offer's data, and it reads an
    image when the offer has no text. GPUI reads the whole text before
    the window can check its size. A fix belongs in GPUI.
- Modifier keys (`browser/engine/modifiers.rs`). Servo keeps one set of
  held modifier keys for all its pages, and every press, release, move,
  and wheel turn that it hands a page carries that set. Only a key event
  changes it: Servo takes the modifiers the event carries, then adds or
  removes the key itself when it is a modifier key. A click on a link
  opens a new window while the set holds Ctrl. GPUI on Linux sends no key
  event for a modifier key, only a change of the keys held. So before
  the fix, Ctrl+C in a page (C let go before Ctrl) left Ctrl in the set
  for every page, also after a reload, and the next plain click on a link
  asked for a new window, which the app refused: the link did nothing.
  - The engine keeps the same set by the same rule, and sends the key-down
    or the key-up of each modifier key that differs from the keys really
    held, to the page that has the keyboard only, and only while no list
    of its `<select>` is open, which keeps its own key handling: as the
    user presses or lets go of Ctrl, Shift, Alt, or Super (as Meta) there,
    in one command with each press, release, and wheel turn of that page
    and the keys that GPUI reports for it, and with no key held when the
    page loses the keyboard in any way or its window stops being the
    active one. A press on a page gives it the keyboard first, then its
    keys. A page so hears the key-down and the key-up of a modifier key as
    in a browser. These key events name no physical key and no side of the
    keyboard (`Code::Unidentified`, `Location::Standard`): GPUI reports
    which modifiers are held, not where. A release while another view,
    tab, menu, or window has the keyboard reaches Servo as a key-up when
    the page loses the keyboard, or at the next press there at the latest.
  - A press, a move, and a wheel turn of Servo carry no keys of their
    own. So before the pointer's event reaches a page that does not have
    the keyboard, the page that has it hears the key-up of each modifier
    key that it holds, and the next input that it takes brings them down
    again. The other page hears no key, and its pointer events find none
    held: Ctrl held in one page of a split and a wheel turn over the other
    page reach that page as a plain wheel turn, without `ctrlKey`, as does
    Ctrl+wheel over a page while the address field has the keyboard. A
    page that closes while it has the keyboard first hears its keys go up.
  - Servo at the pin keeps a press and a later key in order: it tests the
    press's place before `WebView::notify_input_event` returns, and sends
    the press to the constellation on the channel that keys take. Recheck
    this at each change of the pin: if Servo queued the test, a modifier
    let go just after a click could make it a plain click.
- New windows (`browser/engine/popup.rs`). A link with `target=_blank`, a
  Ctrl+click or a middle click on a link, and `window.open` open the page
  as a new tab beside the page that asked, in its workspace and in the
  tab group that the asking tab belongs to there, also while that group
  does not show it, and the window shows that tab. `sheprd browser list`
  lists it like every tab.
  - Servo at the pin asks its host for every new window, with or without
    the user's input: its popup blocker is a TODO (step 8 of
    `choose_a_navigable` in `windowproxy.rs`), and it only consumes the
    page's user activation. So the host admits one new window for each
    press of the user in that page: a button that goes down over it, or a
    key that goes down while it has the keyboard, except a key that
    repeats and Escape. A release, a move, and a wheel turn give nothing
    and renew nothing. The grant lasts 2 s from the press in the window,
    and ends earlier with the page's clipboard grants: as the page starts
    a load, a reload, or a history step, shows another address, hides, or
    loses the keyboard, and as its window stops being the active one. So a
    document that a click leaves for cannot use that click. Any other
    request is refused and logged as before, and `window.open` answers
    `null`: a page cannot open a run of tabs.
  - Servo names the page of a request, not the frame or the script that
    asked. So within those 2 s, any script of the document that got the
    press can use its grant, a timer too, and so can any frame of the
    page. The Walkr section's pages open no window.
  - The new page is the web view that Servo builds from the request, so its
    address, its referrer, `rel=noopener`, and `window.opener` are Servo's.
    The request names no address: Servo builds the page at `about:blank`
    and then navigates it. The engine holds that first navigation, offers
    the page to the window, and lets the navigation go on only once the
    window says that a tab holds the page. A first navigation to another
    scheme or after 10 s, and a page that no tab holds by then (the store
    is full, or the opener's tab is gone), close the page: no new page
    loads its address without a tab, and the new tab never shows a local
    file. The engine checks the same 10 s as the window's word arrives: a
    page that it ended gets no tab, and a word that comes late ends the
    page and closes its tab. A window whose word does not reach the engine
    (a full queue) closes the tab at once. No tab loads the address as a
    page of its own, which would lose the page's request, its opener, and
    a form's data. So `window.open()` with
    no address opens no tab; until those 10 s pass, a script that the
    opener writes into that blank page runs with no tab. The new tab has
    the store, the data folder, and the page rules of every tab, and the
    store's limit of 256 tabs refuses it.
  - The request does not say whether a Ctrl+click or a middle click asked
    for it, so the new tab shows in those cases too, rather than open
    behind the current one. The window features of `window.open` (size,
    place, `popup`) are not applied. The host does not answer Servo's
    `notify_closed`, so `window.close()` in the new tab leaves the tab.
    After a restart the tab opens again at its last address, as every tab
    does, with no opener.
- Not there yet, each with an issue on the tracker: IME, the other
  controls the host must draw (color, file, `alert`,
  `confirm`, `prompt`), context menus, note
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
the middle of a sign-in can finish it. Servo keeps this data in one
folder it is given (`Opts::config_dir`, as its own shell uses it).
The Walkr section's page uses this same folder:

```text
~/.local/state/herdr/gpui/browser-data/     ($XDG_STATE_HOME/herdr/gpui/...)
  cookie_jar.json     every cookie, also those without an expiry
  hsts_list.json      sites that asked for HTTPS only
  localstorage.json, webstorage/            localStorage
  clientstorage/                            IndexedDB
  auth_cache.json                           written by Servo, empty in this app
  cachestorage/                             left by Servo 0.8.0, empty; main makes none
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
- Servo main at the pin reads what 0.8.0 kept, and 0.8.0 reads what
  main writes: checked in the app (debug build, private sandbox) with a
  cookie from a server, one from script, and a `localStorage` value.
  The files keep their names and their format.
- No HTTP cache is kept on disk. Servo main can write its HTTP cache to
  disk when the host asks (`NetworkManager::store_http_cache_to_disk`)
  and its `network_http_disk_cache` preference names a folder; the app
  does neither.
- Other builds are not changed: on macOS and Windows the system's web view
  keeps its own data in its own store.

`sheprd browser clear-data` removes it. The command only opens a dialog in
the window that shows the caller's workspace; nothing goes until the user
confirms there.

- With no Servo page opened in this run, the folder is emptied at once.
- With pages opened, Servo has its storage files open, and Servo (0.8.0,
  and main at the pin) has no call that drops IndexedDB or the HTTPS list from a running engine.
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
- macOS only: WebKit's popups adopted as tabs (`browser/popup.rs`; Servo's
  are adopted through `browser/engine/popup.rs`), WebKit snapshots
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
