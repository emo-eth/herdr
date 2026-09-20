# Tailnet Rollout Runbook: Herdr No-Focus Worktree Fix

## Purpose & Scope

This runbook specifies the repeatable, session-preserving rollout procedure for the Herdr worktree creation no-focus fix across all target desktop and server nodes on the operator's tailnet.

* **Tracking Issue**: [Linear EMO-489](https://linear.app/emo-eth/issue/EMO-489/roll-out-herdr-no-focus-fix-across-tailnet-without-disrupting-sessions)
* **Code Pinned Commit**: [`ebe9ea1574ba9e5c8f0d696b17ce895489342733`](https://github.com/emo-eth/herdr/commit/ebe9ea1574ba9e5c8f0d696b17ce895489342733)
* **Branch**: `bandwidth-upstream` on fork `emo-eth/herdr`
* **Package Identity**: `0.9.1-bandwidth-fix`
* **Patch Description**: Fixes the fallback path in `herdr worktree open --no-focus` / workspace creation when `AppState.active` is `None`, ensuring background worktree and agent operations preserve existing workspace and pane focus without stealing focus from the active user or agent pane. Retains all earlier bandwidth optimization enhancements.

## Identity & Verification Rule

Because this build intentionally retains the package version string `0.9.1-bandwidth-fix`, **the version string alone is insufficient to verify whether the no-focus patch is installed or active.**

Verification requires:
1. **Source commit**: Built strictly from commit `ebe9ea1574ba9e5c8f0d696b17ce895489342733`.
2. **Architecture SHA256 checksums**: Per-architecture cryptographic hashes recorded at build time and verified against candidate and installed disk binaries (`shasum -a 256` on macOS; `sha256sum` on Linux).
3. **Distinct Status Accounting**: A host can have the new binary safely installed on disk while its running server daemon remains on the predecessor build until a scheduled, zero-client maintenance window. Every host is reported with distinct **Installed** and **Running** statuses.

## Target Inventory & Exclusions

### Target Nodes (6 Desktop / Server Hosts)

| Target Name | OS / Architecture | Role / Operating Context |
| :--- | :--- | :--- |
| **`emo-studio`** | darwin-arm64 | Local primary workstation and agent controller |
| **`mbp-16-m4`** | darwin-arm64 | Laptop with active interactive UI client |
| **`spark0`** | linux-arm64 | Remote headless server |
| **`spark1`** | linux-arm64 | Remote headless server |
| **`emo-win`** | linux-x64 (WSL2) | Remote GPU compute host (RTX 5090 host) |
| **`emo-4090`** | linux-x64 (WSL2) | Remote GPU compute host (RTX 4090 host) |

### Explicit Platform Exclusions

* **iOS / iPadOS / visionOS Devices**: iPad, iPhone, and Apple Vision clients on the tailnet run mobile/spatial platforms and do not support Herdr server deployment. They are explicitly excluded from deployment and must **never** be silently counted as deployed.
* **Credentials & Secrets Policy**: No private IP addresses, MAC addresses, Tailscale auth keys, SSH private keys, or pre-shared secrets may be committed or recorded in runbooks or issue trackers.

## Operator Policy: Keep Active Clients Connected

### Architectural Constraints

1. **Client Disconnect on Handoff**: Herdr's Unix live-handoff mechanism transfers PTYs, child processes, workspaces, and registered agents across server instances via private file descriptor transfer. However, **it disconnects all currently attached clients** (both local TUI clients and remote `herdr remote-client-bridge` connections) with no automatic reattachment mechanism.
2. **No Public Client Enumeration API**: Herdr currently exposes no public CLI/API command to query the server's connected client count (`self.clients`), and provides no atomic `--reject-if-clients` guard on `herdr server live-handoff`.
3. **Socket Inspection Limitations**: While OS-level tools can detect active socket streams, process-name searches or the absence of local processes (`ps`) never certify zero accepted unnamed client streams.

### Operating Directive: Install-Only Rollout

* **Default to Install-Only**: This rollout executes **staging and atomic disk installation only** on real sessions.
* **Zero Disruption to Live Sessions**: No server live-handoff or server restart is performed on any production session with active or uncertifiable client attachments. Active sessions remain running on their existing server processes.
* **Preserve Agent & Controller Panes**: Never disrupt or hand off the server hosting the active agent session driving the rollout (`emo-studio`).
* **Future Maintenance Gate**: Runtime activation is deferred to separate, explicitly approved operator maintenance windows where zero attached clients can be certified.

## Safe Rollout Procedure

### Prerequisites & Execution Safety

* **Shell & Tools**: Every procedure snippet requires `bash` and `jq` available in `$PATH`.
* **Strict Shell Mode**: All execution snippets enforce `set -euo pipefail` so that any command failure, pipeline error, or assertion mismatch halts execution immediately and cannot fall through to staging or installation.
* **Smoke Failure Isolation**: If smoke qualification fails at any point, stop and delete **only** the recorded newly created smoke session (`$SMOKE_SESSION`); **never** stop, restart, delete, or signal any production session.

### Phase 1: Isolated Build from Pinned Clean Source

To protect active worktrees, never run `git checkout` or mutate the operator's working checkout. Build within a fresh detached worktree in `/var/tmp`:

```bash
set -euo pipefail

# 1. Create isolated temporary build worktree at pinned commit
WORKTREE_DIR=$(mktemp -d "/var/tmp/herdr-build-ebe9ea15.XXXXXX")
git worktree add --detach "$WORKTREE_DIR" ebe9ea1574ba9e5c8f0d696b17ce895489342733

# 2. Build release binary in isolated worktree
cd "$WORKTREE_DIR"
cargo build --release --locked

# 3. Confirm candidate version string
./target/release/herdr --version
# Expected output: herdr 0.9.1-bandwidth-fix

# 4. Copy candidate to durable, uniquely named versioned staging directory
DIST_DIR=$(mktemp -d "/var/tmp/herdr-dist-ebe9ea15-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m).XXXXXX")
CANDIDATE_BIN="${DIST_DIR}/herdr"
cp -p target/release/herdr "$CANDIDATE_BIN"

# 5. Compute and record expected SHA256 checksum
# macOS:
EXPECTED_HASH=$(shasum -a 256 "$CANDIDATE_BIN" | awk '{print $1}')
# Linux:
# EXPECTED_HASH=$(sha256sum "$CANDIDATE_BIN" | awk '{print $1}')
echo "Candidate SHA256 (${CANDIDATE_BIN}): ${EXPECTED_HASH}"

# 6. Clean up temporary build worktree cleanly (verify path before removal)
cd -
if [ -d "$WORKTREE_DIR" ] && [ -f "$WORKTREE_DIR/.git" ]; then
  git worktree remove "$WORKTREE_DIR"
fi
```

### Phase 2: Isolated Per-Architecture Handoff Smoke Qualification

Before staging production binaries on a host, qualify live handoff and process preservation using an isolated throwaway canary session with a collision-proof name:

```bash
set -euo pipefail

SMOKE_SESSION="smoke-ebe9ea15-$(uname -m)-$$"
# CANDIDATE_BIN is inherited from Phase 1

# Failure trap: ensure failed smoke run cleans up ONLY its own smoke session
trap 'h session stop "$SMOKE_SESSION" 2>/dev/null || true; env -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH -u HERDR_SESSION herdr session delete "$SMOKE_SESSION" 2>/dev/null || true' ERR

# 1. Collision refusal: fail closed if session query fails or session already exists
SESSION_JSON=$(env -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH -u HERDR_SESSION herdr session list --json) || {
  echo "ERROR: Failed to query existing Herdr sessions" >&2
  exit 1
}

if echo "$SESSION_JSON" | jq -e --arg s "$SMOKE_SESSION" '.sessions[]? | select(.name == $s)' >/dev/null; then
  echo "ERROR: Session $SMOKE_SESSION already exists; refusing collision" >&2
  exit 1
fi

# 2. Define isolated CLI wrapper that unsets caller/socket env vars on EVERY call
h() {
  env \
    -u HERDR_SOCKET_PATH \
    -u HERDR_CLIENT_SOCKET_PATH \
    -u HERDR_SESSION \
    -u HERDR_WORKSPACE_ID \
    -u HERDR_TAB_ID \
    -u HERDR_PANE_ID \
    herdr --session "$SMOKE_SESSION" "$@"
}

# 3. Launch isolated background smoke server
env \
  -u HERDR_SOCKET_PATH \
  -u HERDR_CLIENT_SOCKET_PATH \
  -u HERDR_SESSION \
  -u HERDR_WORKSPACE_ID \
  -u HERDR_TAB_ID \
  -u HERDR_PANE_ID \
  herdr --session "$SMOKE_SESSION" server &
SERVER_PID=$!

# 4. Wait for server API readiness (bounded poll on top-level .running)
READY=0
for i in $(seq 1 30); do
  STATUS_OUT=$(h status server --json 2>/dev/null) || true
  if [ -n "$STATUS_OUT" ] && echo "$STATUS_OUT" | jq -e '.running == true' >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 0.2
done

if [ "$READY" -ne 1 ]; then
  echo "ERROR: Smoke server failed to reach ready state within 6s" >&2
  kill "$SERVER_PID" 2>/dev/null || true
  exit 1
fi

# 5. Create test workspace with --no-focus and capture root pane ID dynamically with jq
CREATE_OUT=$(h workspace create --cwd /tmp --no-focus)
PANE_ID=$(echo "$CREATE_OUT" | jq -er '.result.root_pane.pane_id')

# 6. Launch long-lived sentinel process inside pane
# Use sh -c with $$ and exec to avoid shell history expansion ($!) and preserve PID.
# Split marker string in printf to prevent wait-output matching the echoed input command.
h pane run "$PANE_ID" "sh -c 'printf \"%s%s=%d\n\" SNTL RDY \"\$\$\" && exec sleep 3600' &"
h pane wait-output --match "SNTLRDY=" --timeout 5000 "$PANE_ID"

SENTINEL_PID=$(h pane read "$PANE_ID" | grep -o 'SNTLRDY=[0-9]*' | tail -n1 | cut -d= -f2)
case "$SENTINEL_PID" in
  ''|*[!0-9]*)
    echo "ERROR: Invalid sentinel PID captured: '$SENTINEL_PID'" >&2
    exit 1
    ;;
esac
kill -0 "$SENTINEL_PID"

# 7. Perform live handoff to candidate binary with expected version
h server live-handoff --import-exe "$CANDIDATE_BIN" --expected-version "0.9.1-bandwidth-fix"

# 8. Verify post-handoff invariants
# a. Server status running on imported server (top-level .running)
h status server --json | jq -e '.running == true' >/dev/null

# b. Sentinel process PID is preserved exactly across handoff (PTY continuity)
kill -0 "$SENTINEL_PID"

# c. PTY I/O is alive and functional in the pane
h pane run "$PANE_ID" "printf '%s%s\n' POST_HANDOFF_ IO_OK"
h pane wait-output --match "POST_HANDOFF_IO_OK" --timeout 5000 "$PANE_ID"

# 9. Gracefully terminate sentinel process inside the pane and verify process exit
h pane run "$PANE_ID" "kill $SENTINEL_PID; wait $SENTINEL_PID 2>/dev/null || true; printf '%s%s\n' SNTL CLN"
h pane wait-output --match "SNTLCLN" --timeout 5000 "$PANE_ID"

sleep 0.2
if kill -0 "$SENTINEL_PID" 2>/dev/null; then
  echo "ERROR: Sentinel PID $SENTINEL_PID still alive after graceful termination!" >&2
  exit 1
fi

# 10. Cleanly stop and delete only this throwaway smoke session
h session stop "$SMOKE_SESSION"
env -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH -u HERDR_SESSION \
  herdr session delete "$SMOKE_SESSION"
```

### Phase 3: Pre-Install Session Inventory & Process Snapshot

Prior to touching disk binaries on each reachable host, inventory all active sessions and capture their running process IDs:

```bash
set -euo pipefail

# 1. Enumerate all sessions on host
herdr session list --json

# 2. For each active running session, record server socket and verify status
# (Replace <SESSION_NAME> with actual session name)
herdr --session "<SESSION_NAME>" status server --json | jq '{running, socket, version}'

# 3. Capture baseline server PID and child process list
ps aux | grep "[h]erdr"
```

### Phase 4: Staging & Atomic Disk Installation (All Reachable Nodes)

Apply atomic disk replacement. Never overwrite live running binary inodes with `cp`:

```bash
set -euo pipefail

# Explicit required variables provided by operator (no defaults to avoid accidental paths)
: "${INSTALLED_BIN:?INSTALLED_BIN must be set to absolute target path}"
: "${CANDIDATE_BIN:?CANDIDATE_BIN must be set to candidate binary path}"
: "${EXPECTED_HASH:?EXPECTED_HASH must be set to 64-character SHA256}"

# 1. Refuse relative paths
case "$INSTALLED_BIN" in
  /*) ;;
  *)
    echo "ERROR: INSTALLED_BIN must be an absolute path: $INSTALLED_BIN" >&2
    exit 1
    ;;
esac

# 2. Refuse symlinks BEFORE realpath or file checks
if [ -L "$INSTALLED_BIN" ]; then
  echo "ERROR: Refusing to replace symlink directly: $INSTALLED_BIN" >&2
  exit 1
fi

if [ ! -f "$INSTALLED_BIN" ]; then
  echo "ERROR: Target destination $INSTALLED_BIN is not a regular file" >&2
  exit 1
fi

# 3. Checksum candidate binary before proceeding
# macOS:
CANDIDATE_HASH=$(shasum -a 256 "$CANDIDATE_BIN" | awk '{print $1}')
# Linux:
# CANDIDATE_HASH=$(sha256sum "$CANDIDATE_BIN" | awk '{print $1}')

if [ "$CANDIDATE_HASH" != "$EXPECTED_HASH" ]; then
  echo "ERROR: Candidate hash mismatch ($CANDIDATE_HASH != $EXPECTED_HASH); aborting" >&2
  exit 1
fi

# 4. Compute current binary hash and generate unique collision-proof backup
# macOS:
PREV_HASH=$(shasum -a 256 "$INSTALLED_BIN" | awk '{print substr($1,1,12)}')
# Linux:
# PREV_HASH=$(sha256sum "$INSTALLED_BIN" | awk '{print substr($1,1,12)}')

BACKUP_BIN="${INSTALLED_BIN}.bak-$(date +%Y%m%d%H%M%S)-${PREV_HASH}"
if [ -e "$BACKUP_BIN" ]; then
  echo "ERROR: Backup destination $BACKUP_BIN already exists; refusing overwrite" >&2
  exit 1
fi
cp -p "$INSTALLED_BIN" "$BACKUP_BIN"

# 5. Stage candidate to a unique temporary sibling file on the same filesystem
STAGED_BIN=$(mktemp "$(dirname "$INSTALLED_BIN")/herdr.stage.XXXXXX")
install -m 755 "$CANDIDATE_BIN" "$STAGED_BIN"

# 6. Checksum staged candidate before atomic rename (fail closed)
# macOS:
STAGED_HASH=$(shasum -a 256 "$STAGED_BIN" | awk '{print $1}')
# Linux:
# STAGED_HASH=$(sha256sum "$STAGED_BIN" | awk '{print $1}')

if [ "$STAGED_HASH" != "$EXPECTED_HASH" ]; then
  echo "ERROR: Staged binary hash mismatch ($STAGED_HASH != $EXPECTED_HASH); aborting" >&2
  rm -f "$STAGED_BIN"
  exit 1
fi

# 7. Perform atomic replacement via filesystem rename
mv -f "$STAGED_BIN" "$INSTALLED_BIN"

# 8. Assert final installed checksum matches EXPECTED_HASH
# macOS:
FINAL_HASH=$(shasum -a 256 "$INSTALLED_BIN" | awk '{print $1}')
# Linux:
# FINAL_HASH=$(sha256sum "$INSTALLED_BIN" | awk '{print $1}')

if [ "$FINAL_HASH" != "$EXPECTED_HASH" ]; then
  echo "ERROR: Final installed checksum verification failed; rolling back" >&2
  mv -f "$BACKUP_BIN" "$INSTALLED_BIN"
  exit 1
fi
```

### Phase 5: Post-Install Process Verification & Status Recording

Confirm that running server and child processes were untouched during the disk installation:

```bash
# 1. Verify active session is still running
herdr session list --json

# 2. Verify server daemon PID matches pre-install snapshot
# Running daemon PID must be identical to Phase 3 snapshot

# 3. Verify child pane processes remain intact
# Pane process PIDs must be identical to Phase 3 snapshot

# 4. Record host status in Linear EMO-489:
# Target: [node]
# Installed: YES (Commit ebe9ea1574ba9e5c8f0d696b17ce895489342733, hash verified)
# Running: PENDING (Client session preserved / active)
```

### Phase 6: Future Maintenance Activation Gate (Zero-Client Sessions Only)

* **Current Rollout Policy**: No live handoff is executed on active production sessions.
* **Future Operator-Approved Activation**:
  When a scheduled maintenance window occurs and clients are intentionally disconnected:
  1. Retrieve live session socket path from metadata:
     ```bash
     herdr --session "<SESSION_NAME>" status server --json | jq -r '.socket'
     ```
  2. Perform explicit live handoff:
     ```bash
     herdr --session "<SESSION_NAME>" server live-handoff \
       --import-exe "$INSTALLED_BIN" \
       --expected-version "0.9.1-bandwidth-fix"
     ```
  3. Verify new server PID and confirm child pane processes remain intact.

### Phase 7: Rollback & Failure Gates

1. **Stop-the-Line Rule**: If staging, hash verification, or smoke qualification fails on any host, immediately halt the rollout. Do **not** proceed to subsequent machines.
2. **Binary Rollback**:
   ```bash
   mv -f "$BACKUP_BIN" "$INSTALLED_BIN"
   ```
3. **No Automatic Restarts**: Never attempt forced daemon kills, uncoordinated restarts, or looping handoffs on active sessions.
4. **Configuration Stability**: Do not modify release channel configuration in `config.toml`; do not trigger auto-updater checks or downloads.

## Host Reachability & Offline Node Policy

* **RTX 5090 Host (`emo-win` WSL2)**:
  * Node is online.
  * Do NOT remotely sleep, shut down, or power-cycle without explicit operator clearance.
* **RTX 4090 Host (`emo-4090` WSL2)**:
  * Node is online (woken via non-destructive LAN Wake-on-LAN helper).
  * Non-destructive WOL over LAN is the primary wake path. Smart plug power cycling requires explicit clearance and may only be attempted when confirmed safely powered off with no unsaved sessions.
* **Bounded Connection Timeouts**: All remote automation must enforce bounded connection timeouts (`ConnectTimeout=5`) to prevent hangs on transiently unreachable nodes.
* **Explicit Blocker Tracking**: Any host that is unreachable, sleeping, or blocked must be recorded explicitly in Linear EMO-489; it must never be silently marked complete.
