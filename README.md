# NetView

A native TCP connection viewer written in Rust for Windows and Linux, with an
interactive terminal UI powered by Ratatui and Crossterm. It ships as a single
executable; no desktop environment or background service is required.

## Current MVP

- Lists active IPv4 TCP connections.
- Shows local and remote endpoints, connection state, PID, and process name/path.
- Shows connection direction, with explicit markers for snapshot-based estimates.
- Uses the Windows IP Helper API on Windows.
- Reads `/proc/net/tcp` and process file descriptors on Linux.
- Supports periodic refresh with `--watch`.
- Opens a live, scrollable terminal table by default, refreshing every two seconds.
- Provides all-column search, a listener-only toggle, pause/resume, and selected-connection details.
- Sorts every column in ascending or descending order.
- Retains plain-text snapshots for scripts and redirected output.

## Build

Install the stable Rust toolchain from <https://rustup.rs>, then run:

```powershell
cargo build --release
.\target\release\netview.exe
```

On Linux:

```sh
cargo build --release
./target/release/netview
```

The result is one executable for the operating system you build on. Build separately
for Windows and Linux; native binaries are not interchangeable between those systems.

## Usage

```text
netview                 # interactive TUI, refresh every 2 seconds
netview --watch 2       # refresh every 2 seconds
netview --listening     # show listeners only
netview --process chrome
netview --snapshot      # print once and exit
netview > connections.txt # redirected output is a plain snapshot
```

The TUI requires interactive stdin and stdout. Otherwise the program prints one
snapshot. `--watch` requires an interactive terminal and cannot be combined with
`--snapshot`; intervals below one second are clamped to one second.

### Keyboard controls

- `Up` / `Down` or `k` / `j`: select a connection and scroll.
- `Page Up` / `Page Down`: move by ten connections.
- `Home` / `End`: select the first / last connection.
- `1` / `2` / `3` / `4` / `5`: sort by Local / Remote / State / PID / Process.
  `6` sorts Direction; existing sort keys are unchanged.
  Selecting a new column starts ascending; press its key again to reverse direction.
  The active header shows `↑` (ascending) or `↓` (descending). Default: PID ascending.
- `/`: search all columns (local endpoint, remote endpoint, state, PID, direction, and process)
  using a case-insensitive substring; results update as you type.
- `Enter`: keep the filter; `Esc` while editing: restore the previous filter.
- `c`: clear the search and any startup `--process` filter.
- `l`: toggle listening sockets only.
- `Space`: pause or resume automatic collection.
- `r`: refresh immediately, even while paused.
- `q`, `Esc`, or `Ctrl+C`: quit (`q` and `Esc` behave as input/cancel while searching).

The selected-connection panel shows details that may be truncated in the table.
IP addresses, ports, and PIDs sort numerically; state and process sort
case-insensitively. Sorting preserves the selected connection and remains active
across refreshes and filter changes. While editing search, number keys enter text.
`--process` remains process-only and combines with the interactive search and
listener-only toggle. An empty search matches every connection within those filters.
Refresh errors are displayed in the footer while the last successful results remain
available. Counts in the header describe the full snapshot; “shown” reflects filters.
Some processes can only be identified when the program is run with elevated
privileges. On Linux, process ownership also limits which `/proc/<pid>/fd` entries
are visible.

### Connection direction

The Direction column is shown in both the TUI and `--snapshot` output. Direction
means which side initiated the connection, not the direction of individual packets:

- `LISTENING`: a socket accepting connections, not an established connection.
- `INBOUND` / `OUTBOUND`: observed in `SYN-RECV` / `SYN-SENT`, respectively.
- `INBOUND?`: inferred from a matching local listener (address, port, and compatible
  PID; wildcard listeners and unavailable PIDs are supported).
- `OUTBOUND?`: an established connection with a known PID and no matching listener.
- `UNKNOWN`: insufficient information, such as an unowned connection or a closing
  connection first seen after its listener disappeared.

`?` labels are estimates, not authoritative OS metadata. Closed/invisible listeners,
socket ownership changes, shared ports, and simultaneous opens can make inference
ambiguous. No low-port/high-port guessing is used. Observed and inferred roles are
retained across refreshes while the same PID/local/remote tuple remains present,
and discarded when it disappears; tuple reuse between refreshes cannot be detected.
Direction is calculated before UI filters, so filtering out listeners does not
remove their inference context. Search for `inbound`, `outbound`, `listening`, or
`unknown` with `/`; press `6` to sort direction.
## Development

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## Next milestones

1. IPv6 and UDP collection.
2. SQLite connection history and alerts.
3. A Tauri desktop UI over this Rust collection core.
4. Optional firewall integration for explicit, user-approved blocking actions.
