# Browser tabs: web pages and files an agent wrote, shown beside a
# workspace's terminals.

browser-unsupported = Browser tabs need macOS or Windows
browser-no-pages = This build cannot show pages
browser-open-workspace-first = Open a workspace first
browser-too-many-tabs = Too many browser tabs are open

# The address field above a page.
browser-address-placeholder = Enter an address
browser-address-invalid = Not an http or https address

# The button that starts or stops taking notes on the page for an agent.
browser-annotate = Annotate

# What a tab shows where its page is not there yet. $error is why the
# system could not create the page, in its own words.
browser-page-failed = Could not show this page: { $error }
browser-page-empty = Type an address above and press Return.
browser-page-unsupported = This build cannot show pages in the window; open it in the system browser.
browser-page-loading = Loading…

# A line above a page that finished loading with nothing in it, as when a
# site answers with an error and no text, and the button that loads the
# address again.
browser-blank-hint = The site sent an empty page.
browser-blank-reload = Reload
# The same line when the address has a query, which may hold a code that
# works once, as the last address of a sign-in does.
browser-blank-hint-resend = The site sent an empty page. Reload sends this address again, with any one-time code in it.
# The button in place of Reload when the app knows the address the tab was
# asked for on this site. $address is that address.
browser-blank-open = Open { $address }

# Tab titles. A new tab has no address yet.
browser-tab-new = New Tab
# A review of a checkout's changes. $branch is its branch.
browser-tab-review = Review · { $branch }
# An orchestrator tab. $repo is a repository's name.
browser-tab-orchestrator = Orchestrator · { $repo }
browser-tab-devices = Devices
browser-tab-terminal = Terminal

## The bar above a local file, and its switch between page and source.
browser-file-rendered = Rendered
browser-file-source = Source
browser-file-show-source = Show Source
browser-file-unsupported = This build cannot render pages in the window. The file's source can be shown.
browser-file-no-switch = No Markdown or HTML file is in view
browser-file-source-only-build = Source only: this build cannot render pages in the window.
browser-file-source-only-folder = Source only: a page is never served from a hidden folder, or from your home folder itself.
