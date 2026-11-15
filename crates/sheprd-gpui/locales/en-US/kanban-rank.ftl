# Each Kanban lane's order of implementation: a local rank that says what to
# build first. It changes no tracker.
kanban-rank-help = Drag a card up or down in its cell to set the order of implementation. Alt+Shift+Up or Alt+Shift+Down moves the selected card; Alt+Shift+Backspace removes its number. A drop commits; Escape cancels a drag.
# Part of a card's accessible name and tooltip. $number counts from 1.
kanban-rank-detail = Order of implementation: { $number }
kanban-rank-queued = Order of implementation changed here. Saving is asynchronous; no tracker changed.
kanban-rank-unknown = This item does not belong to the lane on the current board.
# Ranks of items no longer shown still count, so the notice names how to free them.
kanban-rank-limit = The order of implementation is full: at most 100 items for each lane and 2,000 in all, including items the board no longer shows. Select a card and press Alt+Shift+Backspace to remove its number, or run `sheprd orchestrator board rank LANE ITEM...` with fewer items to set the lane's whole order.
kanban-rank-invalid = Each item may appear once in the order, with an ID of at most 1,024 bytes.
