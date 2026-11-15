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

type 999 for commands · d dashboard · e exit

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
| `ports` | listening TCP sockets straight from `/proc/net/tcp` (windows: `netstat -ano`) |
| `d` | live status dashboard (ratatui: header + command list + ports, refreshing) |
| `999` | command overview |
| `c` | clear screen |
| `e` | exit |

anything unknown is passed to your real shell, like mova does:

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

## one-shot mode

every command also works from a script or another shell:

```console
$ ava ports            # run once, print, exit
$ ava publicip         # 185.194.66.74
$ ava 0 && echo ok     # exit code of the command is preserved
$ ava --help           # cheat sheet without starting the repl
$ ava --version
```

## dashboard

press `d` (or `ava d`):

- **header** — uptime, load, RAM gauge — refreshes every second
- **left** — the command sheet, `↑↓`/`jk` to move, `enter` to run
- **right** — live listening ports
- `q`/`esc` back to the shell prompt, `enter` inside a run suspends the tui,
  shows the output, and returns when you press enter

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
```

- `[sheets] directories` — directories scanned for `*.toml` command sheets
  (default: `~/.config/ava/sheets`). relative paths resolve against the config
  dir.
- `[sheets] files` — explicit sheet files to load.
- `[interface] color` / `dashboard` — toggles.
- `[history] enabled` — history on/off.

a missing or invalid `config.toml` never breaks ava: defaults are used and a
warning is printed.

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
├── linux.toml
├── network.toml
├── docker.toml
├── virtualization.toml
├── sysadmin.toml
└── homelab.toml
```

copy the ones you need:

```bash
cp examples/linux.toml ~/.config/ava/sheets/
cp examples/docker.toml ~/.config/ava/sheets/
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
- no automatic remote execution, no automatic updates.
- logs and history mask secrets (`TOKEN=...`, `PASSWORD=...`) before writing.

## build

```console
cd ava
cargo build --release
./target/release/ava
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
    ├── repl.rs           # rustyline: history, completion, hints
    ├── dash.rs           # ratatui live dashboard
    └── banner.rs         # banner + help
```

## growing it

add a line to `assets/default.toml`, add a match arm in `cmds.rs::builtin()`, done —
same as mova v1, but the compiler checks everything and `main.rs` stays small.
shell commands don't even need rust: put them in your sheet.
