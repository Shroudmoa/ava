# ava example sheets

Small, practical example sheets for the ava command sheet system — also the
default **mag** catalog: `ava mag list` reads `mags.toml` from this directory
on GitHub, `ava mag get <name>` installs any of them.

Each file is a plain TOML command sheet - copy the ones you like into your
ava sheet directory and they are live immediately, no rebuild needed.

## linux

server basics: disk usage, package updates, reboot.

```console
$ ava disk
$ ava update
$ ava reboot
```

external dependencies: `apt` (or your package manager of choice).

## network

network diagnostics: ping, traceroute, dns, speedtest.

```console
$ ava ping
$ ava traceroute
$ ava dns
```

external dependencies: `ping`, `traceroute`, `dig`, `curl`.

## docker

docker essentials: running containers, images, logs, compose.

```console
$ ava docker-ps
$ ava docker-compose-up
```

external dependencies: `docker`.

## virtualization

libvirt / virtual machine basics.

```console
$ ava vms
$ ava vm-start
```

external dependencies: `virsh`, libvirt.

## sysadmin

systemd and cron operational checks.

```console
$ ava services
$ ava failed
$ ava cron
```

external dependencies: `systemctl`, `crontab`.

## tmux

persistent sessions inside the dashboard's live terminal.

```console
$ ava d          # then select "tmux" and press enter
$ ava tmux-ls
```

mouse clicks and the wheel are forwarded to tmux when it asks for mouse
reporting, so pane resizing and copy mode work normally. `alt+↑↓` scrolls
ava's own view instead.

external dependencies: `tmux`.

## homelab

ssh targets and personal infrastructure.

```console
$ ava nas
$ ava server
$ ava vpn
```

external dependencies: `ssh`, `wg-quick`.

## script-style mags

the `run` key can hold a whole shell script (TOML multi-line string `'''`),
not just a one-liner. three examples:

### healthcheck

full health report in one command: disk, RAM, load, failed services.

```console
$ ava health
$ ava top5
```

external dependencies: `df`, `free`, `systemctl` (optional).

### backup

tar backup of `/etc` with rotation (keeps the last 5). retarget it with
`BACKUP_DIR=/mnt/bak ava backup`.

```console
$ ava backup
$ ava backups
```

external dependencies: `tar`.

### cleanup

conservative maintenance: journal older than 7 days, apt cache, stale /tmp
files — plus a top-10 space-hog report for /var.

```console
$ ava cleanup
$ ava bigdirs
```

external dependencies: `systemctl`/`apt` (optional), `du`.

## install

with ava (downloads from GitHub, validates, installs):

```bash
ava mag get linux network healthcheck
```

without ava (plain curl bootstrap):

```bash
./install.sh linux network healthcheck
```

or the classic copy:

```bash
mkdir -p ~/.config/ava/sheets
cp examples/linux.toml ~/.config/ava/sheets/
cp examples/docker.toml ~/.config/ava/sheets/
```

then restart ava - new names appear in `999`, tab completion and the
dashboard immediately.

## publishing your own mags

1. fork the repo (or make your own), drop `*.toml` sheets next to `mags.toml`
2. add one `[[mag]]` entry per sheet to `mags.toml` (name, file, desc)
3. point ava at it:

   ```toml
   [mags]
   repo = "you/your-mags"
   ```

`ava mag list` now shows your catalog. names must be plain identifiers
(`a-z 0-9 - _`), files must be bare `*.toml` names — anything else is skipped.
