#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/install_systemd_backup.sh [--dry-run] [--enable-now]

Installs the host-systemd Mirror backup timer:
  - builds src/backend maintenance binary in release mode
  - installs it as /usr/local/bin/mirror-maintenance
  - installs infra/systemd/mirror-backup.service
  - installs infra/systemd/mirror-backup.timer
  - installs /etc/mirror/maintenance.env from example if missing

Run as root, or use --dry-run to print actions.

Options:
  --dry-run     Print commands without changing the host.
  --enable-now  Enable and start mirror-backup.timer after install.

After install, edit:
  /etc/mirror/maintenance.env
  /etc/mirror/restic-password
USAGE
}

dry_run=false
enable_now=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      dry_run=true
      shift
      ;;
    --enable-now)
      enable_now=true
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
maintenance_bin="$repo_root/target/release/maintenance"

run() {
  if $dry_run; then
    printf 'DRY-RUN'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

if [[ $EUID -ne 0 && $dry_run == false ]]; then
  echo "must run as root unless --dry-run is set" >&2
  exit 1
fi

run cargo build --release -p mirror-backend --bin maintenance
run install -D -m 0755 "$maintenance_bin" /usr/local/bin/mirror-maintenance
run install -D -m 0644 "$repo_root/infra/systemd/mirror-backup.service" /etc/systemd/system/mirror-backup.service
run install -D -m 0644 "$repo_root/infra/systemd/mirror-backup.timer" /etc/systemd/system/mirror-backup.timer
run install -d -m 0750 /etc/mirror
if [[ ! -f /etc/mirror/maintenance.env || $dry_run == true ]]; then
  run install -m 0640 "$repo_root/infra/systemd/maintenance.env.example" /etc/mirror/maintenance.env
fi
run systemctl daemon-reload
if $enable_now; then
  run systemctl enable --now mirror-backup.timer
fi

cat <<'NEXT'
Installed Mirror backup timer assets.

Next:
  edit /etc/mirror/maintenance.env
  create /etc/mirror/restic-password with mode 0600
  run: systemctl start mirror-backup.service
  run: systemctl status mirror-backup.service
  run: systemctl enable --now mirror-backup.timer
NEXT
