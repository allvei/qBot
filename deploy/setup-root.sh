#!/usr/bin/env bash
# One-time root setup for a qBot host. Run as root: ./deploy/setup-root.sh <service-user>
# Installs the C toolchain needed to build qBot and the qbot@ systemd template.
# Does not enable or start the service; build the bot as <service-user> in ~/qbot first.
set -euo pipefail

SERVICE_USER="${1:?usage: setup-root.sh <service-user>}"
DEPLOY_DIR="$(cd "$(dirname "$0")" && pwd)"

id "$SERVICE_USER" >/dev/null

apt-get update
apt-get install -y build-essential pkg-config

install -m 0644 "$DEPLOY_DIR/qbot@.service" /etc/systemd/system/qbot@.service
systemctl daemon-reload

echo "Done. As $SERVICE_USER: build in ~/qbot, then: systemctl enable --now qbot@$SERVICE_USER"
