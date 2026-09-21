# Herdr Changes (`bandwidth-upstream`)

Tracked fixes and features added to `bandwidth-upstream` (0.9.1-bandwidth-fix) to ensure all changes remain applied and covered against regression.

---

## FIXES

### 1. Worktree Background Creation Focus Preservation (`--no-focus`)
- **Issue**: `herdr worktree open ... --no-focus` and `herdr worktree create ... --no-focus` stole the active workspace, selected tab, and window focus from the operator.
- **Root Cause**:
  1. `src/app/creation.rs`: `create_workspace_with_launch_env` fell back to `switch_workspace` whenever `self.state.active.is_none()` even when `focus` was explicitly false.
  2. `src/app/api/worktrees.rs`: `handle_worktree_open` and deferred setup unconditionally activated newly created workspaces or source parent workspaces.
   3. `src/server/headless/client_views.rs`: `inspect_worktree_open` was guarded by `params.focus`, but underlying workspace switches triggered view reconciliation.
   4. `~/.config/herdr/plugins/config/cloudmanic.herdr-plus/bin/focus-omp.sh`: Worktree startup hook layout (`all.toml`, `simon.toml`, `skills.toml`) ran `herdr agent focus` unconditionally upon OMP startup, which issued an `agent.focus` API call 2 seconds after worktree creation and yanked the operator's viewport to the newly created workspace.
 - **Fix**:
   - `src/app/creation.rs`: strictly check `if focus` before calling `switch_workspace`.
   - `src/app/api/worktrees.rs`: preserve active workspace, active tab, pane focus, and mode when `focus` is false across linked worktree opening, source parent creation, and existing checkout resolution.
   - `~/.config/herdr/plugins/config/cloudmanic.herdr-plus/bin/focus-omp.sh`: added `is_focused` guard checking workspace focus before invoking `agent focus`; background worktrees exit immediately without stealing focus.
   - Commits: `ebe9ea15` (core Herdr fix)
- **Regression Tests**:
  - `api::worktrees::tests::api_worktree_open_no_focus_preserves_none_active_and_mode`
  - `api::worktrees::tests::api_worktree_open_no_focus_preserves_connected_client_location_when_active_is_none`
  - `api::worktrees::tests::api_worktree_open_creates_source_parent_without_stealing_focus`
  - `server::headless::tests::worktree_open_no_focus_preserves_connected_client_location_when_active_is_none`

---

### 2. Selection Retention Across Mouse-Up and Prefix Key
- **Issue**: Mouse drag-selection cleared immediately on `MouseUp` when `copy_on_select = true` and was also cleared when entering prefix mode (`Ctrl+B`). As a result, plugin actions like `herdr-annotate` (`prefix+a`) received an empty `selected_text` in `HERDR_PLUGIN_CONTEXT_JSON` and failed with *"Nothing to annotate: Select text in Herdr or copy text to the clipboard"*.
- **Root Cause**:
  - `src/client/shell/mouse.rs`: `handle_mouse_up` set `self.selection = None` immediately after `request_selection_copy`.
  - `src/client/shell/input.rs`: `self.selection.take()` was called before checking whether the key entered prefix mode.
- **Fix**:
  - Retain `self.selection` on mouse-up even after copy.
  - Preserve `self.selection` during prefix key evaluation; clear only when normal typing occurs.
  - Commits: `06bdfaf4`
- **Regression Tests**:
  - `client::shell::tests::mouse_selection::client_selection_retained_across_mouse_up_and_prefix_and_delivered_to_plugin_action`
  - `client::shell::tests::mouse_selection::mouse_drag_selection_*` (7 tests)

---

### 3. Hyperlink Activation Over Remote Client Shell
- **Issue**: Ctrl-click / pane link activation over remote shell needed verification to ensure URLs resolve and pass through to viewing clients without focus stealing or state corruption.
- **Verification**:
  - Added unit test `pane_link_activate_returns_url_and_handled` in `src/app/api/plugins/mod.rs`.
  - Added integration test `client_shell_pane_link_activate_resolves_and_activates` in `src/server/headless/tests/mod.rs`.
  - Commits: `4c6b0c78`
- **Regression Tests**:
  - `app::api::plugins::tests::pane_link_activate_returns_url_and_handled`
  - `server::headless::tests::client_shell_pane_link_activate_resolves_and_activates`

---

## FEATURES

### 1. Remote Client Clipboard Routing (`herdr clipboard set`)
- **Requirement**: Over remote SSH connections (e.g. `mbp-16-m4` -> `studio`), plugin actions like `herdr-annotate` copy markdown to the clipboard. The server had no direct access to the client OS pasteboard, failing or writing to the remote host.
- **Implementation**:
  - Added `Method::ClientClipboardSet` API schema (`src/api/schema/common.rs`, `src/api/schema/response.rs`).
  - Added `herdr clipboard set --stdin` CLI command (`src/cli/clipboard.rs`).
  - Server routes `ClientClipboardSet` directly across the client shell control stream to the foreground viewing client (`src/server/headless.rs`).
  - Commits: `06bdfaf4`
- **Regression Tests**:
  - `server::headless::tests::client_clipboard_set_requests_round_trip`
  - `server::headless::tests::client_shell_client_clipboard_set_routes_to_foreground_client`
  - `server::headless::tests::client_shell_client_clipboard_set_without_foreground_client_rejects`
