# ava

**mova concept, linux edition — written in rust.**

Short names, long reach. One REPL, a sheet of commands, everything else is
plumbing. If you know `mova2.ps1`, you already know ava.

```console
$ ava
      __          root@ti-gw
   /\|  |/\  ava  Uptime : 4d 23h 11m
  /          \    Load   : 0.50 0.42 0.20
 /  \____/\  \    RAM    : 1.5/3.8 GB |||....... 38%

visionmaxx · type 999 for commands · d dashboard · mag mags · e exit

ava::ti-gw@root ~/ViS › _
```

## why rust and not powershell

- **one static binary** — `scp ava box:/usr/local/bin/` and it runs. no interpreter,
  no execution policy, no `Add-Type`, no CIM queries.
- **reads the kernel directly** — listening ports come from `/proc/net/tcp`,
  not from parsing `Get-NetTCPConnection` output.
- **data-driven sheet** — commands live in TOML files. add one in 10 seconds,
  no rebuild (same idea as mova's `$cheatSheet`, but it can't drift out of sync).
- **cross-platform** — same codebase, `cfg`-gated windows arms
  (`ipconfig`, `route print`, `netstat -ano`, `cmd /C`).

## commands

| name | what it does |
|------|--------------|
| `0` | system info — OS, kernel, uptime, load, RAM with bar (green/yellow/red) |
| `2` | network info — `ip addr` + `ip route` (windows: `ipconfig /all` + `route print`) |
| `ports` | listening TCP sockets from `/proc/net/tcp{,6}` with decoded IPv4/IPv6 addresses (windows: `netstat -ano`) |
| `d` | live status dashboard (ratatui: header + command list + terminal, refreshing) |
| `999` | command overview |
| `mag` | command mags from GitHub — `list`, `get <name>`, `update`, `rm <name>` |
| `c` | clear screen |
| `e` | exit |

anything unknown is passed to your real shell, like mova does — with a real
terminal attached, so interactive programs (`ssh`, `vim`, `top`, ...) work:

```console
ava::ti-gw@root ~ › ls -la | head -3
total 12
drwxr-xr-x 3 root 4096 .
...
```

## your own commands — no rebuild

create `~/.config/ava/sheets/local.toml` (or `~/.ava/sheet.toml` for the classic
single-sheet location):

```toml
[[cmd]]
name = "10"
desc = "Open myvision portal"
kind = "shell"
run  = "xdg-open https://myvision.framer.website/ >/dev/null 2>&1"
win  = "start https://myvision.framer.website/"     # optional windows override

[[cmd]]
name = "reboot-hint"
desc = "How to reboot this box"
kind = "shell"
run  = "echo sudo reboot"
```

that's it. new names appear in `999`, in tab-completion and in the dashboard
immediately. entries with the same name **shadow** the built-in ones
(override `0` if you want your own sysinfo).

## mags — command configs from GitHub

a **mag** is a command sheet published in a GitHub repo. `mag` pulls them
straight into your sheets dir — no browser, no copy-paste:

```console
$ ava mag list              # the repo's catalog (+ marks installed ones)
$ ava mag get healthcheck   # download, validate, install
$ ava mag update            # re-fetch every installed mag
$ ava mag rm healthcheck    # remove one
```

the default repo is [`Shroudmoa/ava`](https://github.com/Shroudmoa/ava), path
`examples/`, branch `main`. the repo keeps an index — `mags.toml` — next to the
sheets:

```toml
[[mag]]
name = "healthcheck"
file = "healthcheck.toml"
desc = "Script-style: full health report"
```

point ava at your own repo (or a fork) in `config.toml`:

```toml
[mags]
repo   = "you/your-mags"   # owner/repo
branch = "main"
path   = "examples"        # dir holding mags.toml + the sheets ("" = repo root)
```

production guards: downloads are https-only via `curl`, every mag is parsed as
a valid sheet **before** it is installed, writes are atomic (tmp + rename), and
index names/paths are validated against traversal. installed mags load on the
next ava start. and remember: mags run shell commands with your permissions —
only install from repos you trust.

no ava on the box yet? `examples/install.sh` bootstraps mags with plain curl:

```bash
./install.sh linux network healthcheck
```

## one-shot mode

every command also works from a script or another shell:

```console
$ ava ports            # run once, print, exit
$ ava publicip         # 185.194.66.74
$ ava echo "a  b"      # a  b  (argv is re-quoted, spacing survives)
$ ava 0 && echo ok     # exit code of the command is preserved
$ ava --help           # cheat sheet without starting the repl
$ ava --version
```

one-shot mode prints only the command's output — no "trying shell..." notice.
the exit code of the command is preserved.

## dashboard

press `d` (or `ava d`):

- **header** — two live panels: host/uptime/load/RAM gauge and
  CPU/disk/network-rates/process count. heavy stats refresh once a second.
- **commands** — the command sheet, `↑↓`/`jk` to move, `pgup`/`pgdn` by page,
  `enter` to run
- **one little terminal** — a real interactive shell in a pseudo-terminal, right
  under the list. `ctrl+t` focuses it (spawning the shell on first use); it keeps
  its own cwd, env, exports and history, and ANSI output (colors, prompts,
  cursor movement) is emulated inside the pane. the pty is resized to the exact
  pane size: focused, it uses the whole body instead of the narrow list-view
  pane. ava does not interpret what you type — keystrokes go straight to the
  shell as raw bytes.
- **scrolling** — mouse wheel over the pane, or `alt+↑`/`alt+↓` (page:
  `alt+pgup`/`alt+pgdn`, `alt+home`/`alt+end`) browses 5000 lines of scrollback.
  typing or clicking follows the prompt again. plain `pgup`/`pgdn` still go to
  the shell, so tmux and `less` keep their own bindings.
- **mouse-aware programs** — when the shell asks for mouse reporting (tmux, vim,
  less, ...) clicks and the wheel are forwarded in its preferred encoding;
  otherwise the wheel scrolls ava's view. see `examples/tmux.toml`.
- **`/` search** — type to filter the list live (name + description, case
  insensitive), `↑↓`/`jk` move through the matches, `enter` runs the top hit,
  `esc` clears the search
- **jump list ⇄ terminal** — `ctrl+t` toggles between the command list and the
  terminal view. `tab` is **not** a dashboard keybinding — in the terminal it is
  passed through to the shell for normal tab-completion.
- selecting a `kind = "shell"` command types it into the live terminal;
  ava builtins suspend the tui, run, then return.
- `q` (in the list) exits; `esc` returns from the terminal to the list (the shell
  keeps running in the background); `exit`/`ctrl-d` end the shell itself.
- the dashboard needs an interactive terminal: from a pipe or script it prints a
  clear error and exits with code 1 instead of crashing.

## the usual mova glue

- colored prompt: `ava::host@user ~/path › `
- every input logged (secrets masked) - see [history](#history)
- history in `~/.config/ava/history` (↑/↓), tab-completion of all sheet names,
  inline grey hint when a command name is uniquely matched
- `ctrl-c` cancels the line, `ctrl-d` exits

## configuration

ava works with **no configuration file at all**. when none is found it uses
sensible defaults.

```
~/.config/ava/
├── config.toml     # optional
├── sheets/         # optional: scanned for *.toml sheets
└── history         # shell history
```

minimal `config.toml`:

```toml
[sheets]
directories = []
files = []

[interface]
color = true
dashboard = true

[history]
enabled = true

[shell]
name = "bash"      # shell for the dashboard terminal: sh | bash | fish | zsh

[mags]
repo   = "Shroudmoa/ava"   # GitHub repo `mag` pulls from
branch = "main"
path   = "examples"        # dir holding mags.toml + the sheets
```

- `[sheets] directories` — directories scanned for `*.toml` command sheets
  (default: `~/.config/ava/sheets`). relative paths resolve against the config
  dir.
- `[sheets] files` — explicit sheet files to load.
- `[interface] color` — when `false`, ANSI colors are stripped from the banner,
  help, prompt, sysinfo and dashboard (plain text everywhere).
- `[interface] dashboard` — when `false`, the `d` command refuses to start and
  explains why; all other commands are unaffected.
- `[history] enabled` — when `false`, history is neither read, appended nor saved.
- `[shell] name` — the independent shell spawned inside the dashboard terminal
  (`bash` by default). any installed shell works: `fish`, `bash`, `zsh`, `sh`.

  ```toml
  [shell]
  name = "fish"   # or "bash" / "zsh" / "sh"
  ```

  the dashboard terminal is a real PTY session of that shell — not an exec
  wrapper and not ava's own dispatch. builtins (sysinfo, ports, dash, ...) are
  ava's own and always run as ava. ava's ordinary unknown-command fallback shell
  is separate and unchanged.
- `[mags] repo / branch / path` — where `mag` downloads command configs from
  (default `Shroudmoa/ava`, `main`, `examples`). invalid values fall back to the
  defaults with a warning. see [mags](#mags--command-configs-from-github).

a missing or invalid `config.toml` never breaks ava: defaults are used and a
warning is printed (including when the file exists but cannot be read). the
config is read once at startup and shared by every subsystem, so sheets,
colors, history and the dashboard always agree.

## multiple sheets, one command set

ava loads, in order:

1. the embedded builtin sheet (`0`, `2`, `ports`, `d`, `999`, `c`, `e`, ...)
2. user sheets - legacy `~/.ava/sheet.toml`, then configured sheet
   directories (sorted, `*.toml` only), then configured files, then `./sheets`
   relative to the current directory.

later sheets **override** earlier ones, exactly like a user shadowing a builtin.
within one sheet, later entries with the same name override earlier ones.

a broken sheet is reported and skipped - it never silently corrupts the rest.

## example sheets

see [`examples/`](examples/):

```
examples/
├── README.md
├── mags.toml           # the mag index `ava mag list` reads
├── install.sh          # curl bootstrap: install mags without ava
├── linux.toml
├── network.toml
├── docker.toml
├── virtualization.toml
├── sysadmin.toml
├── tmux.toml
├── homelab.toml
├── healthcheck.toml    # script-style: whole scripts in one run key
├── backup.toml         # script-style: tar backup with rotation
└── cleanup.toml        # script-style: journal / cache / tmp cleanup
```

copy the ones you need:

```bash
cp examples/linux.toml ~/.config/ava/sheets/
cp examples/docker.toml ~/.config/ava/sheets/
```

or let `mag` do it: `ava mag get linux docker`.

**script-style mags** (`healthcheck`, `backup`, `cleanup`) show the other shape
of a command config: the `run` key is a whole shell script in a TOML multi-line
string, not a one-liner —

```toml
[[cmd]]
name = "backup"
desc = "Backup /etc, keep the last 5"
kind = "shell"
run = '''
set -eu
dir="${BACKUP_DIR:-$HOME/backups}"
mkdir -p "$dir"
stamp="$(date +%Y%m%d-%H%M%S)"
tar -czf "$dir/backup-$stamp.tar.gz" /etc 2>/dev/null || true
ls -1t "$dir"/backup-*.tar.gz | tail -n +6 | while read -r old; do rm -- "$old"; done
'''
```

## distribution

```
IT SERVER
│
├── ava
│
└── sheets/
    ├── default.toml
    ├── linux.toml
    ├── network.toml
    ├── virtualization.toml
    └── examples/
        ├── sysadmin.toml
        ├── docker.toml
        └── homelab.toml
```

download the binary and whichever command sheets you need:

```bash
curl -O https://example.com/ava
curl -O https://example.com/sheets/linux.toml

chmod +x ava
./ava
```

`./sheets/` next to the binary is picked up automatically when you run ava from
there.

## security and reliability

- TOML shell commands run with the permissions of the current user - that is the
  whole point of the design, there is no sandbox to escape.
- never run sheets from people you don't trust: they execute shell commands.
  same goes for mags — `mag` only downloads from the repo you configured, over
  https, and validates every sheet before installing it, but the commands inside
  still run as you.
- no automatic remote execution, no automatic updates. `mag` only ever fetches
  when you ask it to.
- logs and history mask secrets before writing: `TOKEN=...`, `PASSWORD=...`,
  `--token value`, `Bearer value` and `Authorization: Bearer value`. single-dash
  flags (`-psecret`) are left alone so normal commands stay readable.
- the log rotates to `ava.log.1` once it passes 1 MiB.

## build

```console
cd ava
cargo build --release
./target/release/ava
cargo test          # unit tests + end-to-end CLI tests in tests/cli.rs
```

windows arm is written against `netstat -ano`, `ipconfig /all`, `route print`
and `cmd /C` — built for windows in the same way, but only smoke-tested on
linux so far.

## layout

```
ava/
├── Cargo.toml
├── assets/default.toml   # the embedded default sheet
├── examples/             # example command sheets
└── src/
    ├── main.rs           # entry: repl vs one-shot
    ├── config.rs         # platform config dir, defaults, optional config.toml
    ├── sheet.rs          # load + merge builtin and user sheets
    ├── cmds.rs           # builtins + dispatch + logging
    ├── mag.rs            # mag: fetch command configs from GitHub
    ├── color.rs          # global color switch from config
    ├── repl.rs           # rustyline: history, completion, hints
    ├── dash.rs           # ratatui live dashboard
    └── banner.rs         # banner + help
```

## growing it

add a line to `assets/default.toml`, add a match arm in `cmds.rs::builtin()`, done —
same as mova v1, but the compiler checks everything and `main.rs` stays small.
shell commands don't even need rust: put them in your sheet.
