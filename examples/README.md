# ava example sheets

Small, practical example sheets for the ava command sheet system.

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

## homelab

ssh targets and personal infrastructure.

```console
$ ava nas
$ ava server
$ ava vpn
```

external dependencies: `ssh`, `wg-quick`.

## install

```bash
mkdir -p ~/.config/ava/sheets
cp examples/linux.toml ~/.config/ava/sheets/
cp examples/docker.toml ~/.config/ava/sheets/
```

then restart ava - new names appear in `999`, tab completion and the
dashboard immediately.
