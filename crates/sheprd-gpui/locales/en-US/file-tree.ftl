# The file explorer: a panel on the window's right with the folders of the
# focused pane's checkout as a tree.

# The panel's name, read by a screen reader.
file-tree-title = Files
# Placeholder of the field that lists files by name.
file-tree-search = Search files
# Buttons beside the search field, read by a screen reader.
file-tree-refresh = Read the folders again
file-tree-collapse = Close all folders
# Shown in place of the tree.
file-tree-loading = Reading...
file-tree-searching = Searching...
file-tree-no-match = No matching files.
file-tree-no-folder = Focus a terminal to list the files of its folder.
file-tree-remote = Files are not available for a remote host yet.
# A row under a folder that holds more entries than the tree lists.
file-tree-more = { $count ->
    [one] { $count } more entry is not listed.
   *[other] { $count } more entries are not listed.
}
# The same row for a folder too large to count all it holds.
file-tree-more-at-least = At least { $count } more entries are not listed.
# The last row of a tree with more rows than the panel shows.
file-tree-cut = More entries are not listed.
# Shown when the user opens one folder too many.
file-tree-too-many-open = Close a folder before you open another.
# Shown when the user chooses a symbolic link.
file-tree-link = Links are shown and not followed.
