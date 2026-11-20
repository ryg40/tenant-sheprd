---
title: Code and editors
blurb: Jump to code, split the view, open VS Code or px0.
status: shipped
---

# Code and editors

Sheprd is not an editor, but it gets you to the right line next to the agent that is changing the code. This page covers opening files in your editor, Go to Symbol and Go to File, the file explorer, read-only code tabs, editor groups, and VS Code and px0 tabs.

## Open in your editor

Cmd-click (Ctrl-click on Linux and Windows) a printed path such as `src/main.rs:42` to open it at that line in your terminal editor, in a new pane split to the right of the pane that printed it. In a [review](review.md), `e` or a file header's pencil opens the file the same way. Quitting the editor closes its pane.

The editor is `$VISUAL`, then `$EDITOR`, then `vi`, as the new pane's shell sees them, started as `EDITOR +LINE FILE`. Set `editor_command` to choose another, with `{file}` and `{line}` placeholders:

```toml
editor_command = "hx {file}:{line}"
```

Placeholders may sit inside double quotes, as in `subl "{file}:{line}"`. A file whose path contains a quote, backslash or control character opens in the system's application instead.

When the editor is Neovim, later files for the same tab open in that Neovim instead of a new split, so `nvim` must be on the app's `PATH`. If that Neovim has quit, the file opens in a new split; if it is busy, such as on a swap-file prompt, its pane comes forward. An `editor_command` that uses `{file}` or `{line}` runs exactly as written and is never reused.

To open printed paths in the system's default application instead, set `open_files_in = "system"`. Holding Alt (Option on macOS) while clicking opens a file the other way. See [Terminals](terminals.md#terminal-links).

> **Windows.** A pane's shell has no `sh` to start the editor with, so printed paths open in the system's application, and Go to Symbol and Go to File open code tabs.

## Go to Symbol and Go to File

**Go to Symbol** lists the functions, types, modules and constants of the focused pane's checkout. **Go to File** lists its files, and takes `file:line` to open at a line. Tab switches between the two.

| Command | macOS | Linux and Windows |
| --- | --- | --- |
| Go to Symbol | Cmd-Shift-O | Ctrl+Alt+Shift+O |
| Go to File | Cmd-O | Ctrl+Shift+O |

Before you type, both start with what the agent is changing: the checkout's uncommitted files, newest first, with their `+added −removed` line counts, or the definitions in them. Enter opens the result in your editor beside the focused pane; Cmd-Enter (Ctrl-Enter elsewhere) opens it in a code tab instead.

The symbol index is built in the background from the files Git lists, and refreshed when older than 30 seconds. It reads definitions from each line's leading keywords for Rust, Swift, Ruby, Python, Go, JavaScript and TypeScript, Kotlin, Java, C#, Elixir, Zig, and the types and macros of C-family files. It is a heuristic, not a language server: macro-generated definitions are missed, and C and C++ functions are not listed. Only local checkouts are indexed.

The same search works from a pane, without the app running:

```sh
sheprd search symbol QUERY
sheprd search file QUERY
```

## File explorer

The file explorer is a panel on the window's right. It shows the folders of the focused pane's checkout as a tree: the folder above the pane's folder that holds `.git`, or the pane's folder when it is in no checkout. The root always holds the pane's folder: a `core.worktree` setting of the repository does not move it. Open and close it with **Toggle File Explorer** in the command palette. Bind `toggle_file_explorer` under `[keybindings]` for a key; it has none by default.

Click a folder to open or close it. Click a file to open it in the view that fits it; see [Files opened from the tree](#files-opened-from-the-tree). Type in **Search files** to list the checkout's files by name instead, ranked as Go to File ranks them; this needs a Git checkout. The list follows the text a moment after you stop typing.

| Key | Action |
| --- | --- |
| Up / Down | Move through the rows; Up on the first row goes to the search field |
| Right | Open the folder, or step into an open one |
| Left | Close the folder, or go to the folder holding the row |
| Enter | Open the file, or open or close the folder |
| Escape | Clear the search, then return to the terminal |

The row above the tree names the folder it lists; hover it for the whole path. The tree is read when the panel opens and when you press the refresh button; it does not watch the disk. A folder that is gone from the disk closes on refresh. The button beside the search field closes every folder. When the focus moves to a pane in another checkout, the tree shows that checkout; the folders you opened in the first one open again when the focus comes back to it. The tree keeps them for the last 16 checkouts, until Sheprd quits. Drag the panel's left edge to resize it; the width is kept when Sheprd restarts.

The terminals always keep 240 pixels of width. When the window is too narrow for that, the mod panel (the column of mod sections) yields first: it narrows, then steps aside. Only then does the file explorer narrow, and step aside in turn. Both come back when the window is wide enough.

### Files opened from the tree

A Markdown or HTML file gets the same view from the tree as from `sheprd browser open FILE` or the switch of a tab. One rule decides it:

| File | View |
| --- | --- |
| Markdown or HTML (`.md`, `.markdown`, `.html`, `.htm`) | The rendered page, in a [browser tab](browser.md#local-pages). An HTML page runs its own scripts |
| Markdown or HTML in a hidden folder (such as `.local/`), or directly in your home folder | Its source, read-only. The bar above it says why it has no page |
| Markdown or HTML, in a build that cannot render pages (Linux without the `servo` feature) | Its source, read-only. The bar says why |
| Any other file | A [code tab](#code-tabs) |

For any other file, the two differ. The tree shows it as code. `sheprd browser open FILE` shows it as a page, because the caller asked for a page, and refuses it in a hidden folder.

A Markdown or HTML file has [the file bar](browser.md#the-file-bar) above it, as a page and as source: a breadcrumb of its path, and the switch between **Rendered** and **Source**. Click a folder of the breadcrumb to show the file explorer with that folder open. This works for a folder that the explorer lists; the file's own name is not a link.

The tree marks the file that the active tab shows, whether the tab shows its page, its source, or its code.

### What the tree lists

- Files Git ignores are listed, dimmed. `.git` is never listed.
- A symbolic link is listed and never followed, so nothing outside the checkout is read.
- A folder lists its first 2,000 entries by name, of the first 8,000 it reads, and a row under them says how many more it holds: "at least" that many when it holds more than 8,000. At most 256 folders are open at once, and a refresh reads four folders at a time.
- A name with a colon or a backslash is listed and opens on macOS and Linux. On Windows, where they separate names and drives, such a path is refused.
- A name with a control character or a text direction override, or longer than 255 bytes, is not listed; the folder counts it among the entries not listed.
- Each folder on the way to a file is checked to be a real folder before it is read. A program that swaps a folder for a link between the check and the read can make a listing show names from outside the checkout; it needs write access to the checkout to do so.
- On Windows, `sheprd files reveal` and the mark on the active tab's file may not find a path, because the resolved path and Git's path are written differently. Windows and macOS are not verified yet.
- Only local checkouts are listed. A workspace on an SSH or cloud host shows "Files are not available for a remote host yet."

Agents and scripts use `sheprd files show|hide|toggle`, and `sheprd files reveal PATH` to select a file in the tree. These show the panel and leave the keyboard in the terminal; the palette command gives the panel the keyboard. The tree is that of the window's focused pane, so `reveal` refuses a file of another checkout.

## Code tabs

A code tab shows a file read-only, with syntax coloring and an outline of its definitions, beside the workspace's terminals like any other tab. Agents open one with `sheprd code open FILE[:LINE]`; `sheprd code edit FILE[:LINE]` opens your terminal editor instead.

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | Scroll |
| Space, Page Up / Page Down | Page |
| `g` / `G` | Top / bottom |
| `o` | Show or hide the outline |
| `e` | Open the marked line, or the top line in view, in your editor |

Click an outline entry to jump to it, or a line to mark it. Code tabs read files up to 1 MiB and color their first 20,000 lines.

## Editor groups

The split button at the right end of the tab strip, or **Split Editor** (`cmd-\` on macOS, `ctrl-shift-\` on Linux and Windows), splits the view into side-by-side groups, as an editor does. Keep an agent's terminal in one group and its page or review in another. Each split opens a new, empty group to the right; splitting moves no tab.

- A tab belongs to one group. A tab opened in a group is that group's own. A tab opened elsewhere, by an agent, the CLI or another client, goes to the group in use.
- Drag a tab onto another group to move it there. A group closes when its last tab leaves.
- Groups show different Herdr tabs live, side by side. The group in use has the keyboard and its chosen tab carries the accent; click a group to use it.
- **+** opens a terminal tab in the group. The group's **…** menu offers **New Browser Tab**, **Review Changes**, **VS Code**, **px0**, the workspace's listening ports, **Split Right**, and, once the view is split, **Close Tab**, **Close Others** and **Close Group**. **Close Group** closes no tab: its tabs go to the group that takes its room.
- Drag a divider to resize the groups beside it.
- Each workspace's groups, their tabs and widths, and the group in use come back after a restart.

Closing a tab closes it for good, split or not. A Herdr tab closes in Herdr, through its usual confirmation.

## VS Code

VS Code opens as a tab of a workspace, for reading code and reviewing diffs while agents work in the terminals. The page comes from VS Code's own web server, `code serve-web`.

> **Platforms.** VS Code tabs work on macOS and Windows. Linux builds show no embedded pages, so they never start VS Code.

Open it with **VS Code** in an editor group's **…** menu, or **Open VS Code** in the Terminal menu or the command palette. Bind `open_code` under `[keybindings]` for a key; it has none by default. Each workspace has one VS Code tab: opening it again moves that tab to the group you opened it from.

**Settings > Code** chooses where the server comes from:

- **Start VS Code automatically**, the default once VS Code is found, runs the server for you. It looks for VS Code's `code` command on your login shell's `PATH`, then, on macOS, in `/Applications/Visual Studio Code.app`. The server is Microsoft software under the [VS Code Server License Terms](https://aka.ms/vscode-server-license); Sheprd never accepts them for you, so the tab first offers **Accept and start**. The first start downloads the server. Sheprd stops the server when it quits, and restarts it if it fails.
- **Use an address** keeps a server you run yourself:

```sh
code serve-web --host 127.0.0.1 --port 8000 --accept-server-license-terms
```

It prints an address with a `?tkn=` connection token. Paste it in **Settings > Code**, which saves it:

```toml
[code]
mode = "address"
url = "http://127.0.0.1:8000/?tkn=..."
```

Keep the port fixed: VS Code keeps its settings in the page's storage, which belongs to one address. **Test connection** checks that the server answers.

A workspace's VS Code page first opens the folder the workspace started in, when it is on this computer. A workspace on an SSH host opens an empty window, since its folder is on another machine. While a VS Code page has the keyboard, its own shortcuts win; the keys for switching tabs and workspaces still reach Sheprd. Click a terminal to take the keyboard back.

## px0

**Open px0** in the Terminal menu or command palette, or **px0** in an editor group's **…** menu, opens [px0](https://px0.ai/) on the local workspace's starting directory, without the browser toolbar. Each workspace has one px0 tab. `open_px0` can be bound in keybindings; it has no default shortcut. Agents open it with `sheprd px0 open`.

You need a px0 build with the review workbench. Sheprd finds a running workbench, or looks for `px0` (`px0.exe` on Windows) on your login shell's `PATH` and starts it when a tab first needs it. It never installs or updates px0. Quitting Sheprd stops a workbench it started, but never one it found already running.

To use a workbench you run yourself, enter its address in **Settings > Code > px0** and press **Apply px0 address**, or:

```toml
[px0]
url = "http://127.0.0.1:7777/"
```

Only loopback HTTP addresses are accepted. Leave it empty for automatic discovery. Startup failures appear in the tab with **Retry**.

> **Note.** px0 tabs support local workspaces on macOS and Windows. Remote workspaces and Linux are not supported.
