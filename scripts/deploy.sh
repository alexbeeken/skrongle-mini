#!/usr/bin/env bash
# One-command deploy: rsync the binary + data to the Pi, install the systemd
# unit on first run, restart the service.
#
#   scripts/deploy.sh pi@raspberrypi.local            # aarch64 build
#   scripts/deploy.sh pi@<pi-lan-address> armv7
#
# Assumes the Pi user can sudo (default on Raspberry Pi OS).
# Honours SSH_OPTS (e.g. "-i ~/.ssh/localpi -o IdentitiesOnly=yes"; release.sh sets it
# from PI_SSH_KEY in deploy/deploy.env), PI_BASE_PATH and PI_PUBLIC_URL, and
# writes /etc/spanish-learning/env on the Pi the first time.
set -euo pipefail
cd "$(dirname "$0")/.."
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=15}"
# shellcheck disable=SC2086
SSH() { ssh $SSH_OPTS "$@"; }
RSYNC() { rsync -e "ssh $SSH_OPTS" "$@"; }
PI_BASE_PATH="${PI_BASE_PATH:-/spanish}"
# The public URL lives in deploy/deploy.env (release.sh loads it), not in the repo.
: "${PI_PUBLIC_URL:?set PI_PUBLIC_URL (e.g. in deploy/deploy.env): the https URL the app is reached at}"
case "$PI_PUBLIC_URL" in https://*|http://*) ;; *) echo "PI_PUBLIC_URL must start with https:// (got $PI_PUBLIC_URL)" >&2; exit 1 ;; esac
# scheme://host[:port], for the nginx redirect.
PI_PUBLIC_ORIGIN=$(printf '%s' "$PI_PUBLIC_URL" | sed -E 's|^([a-z]+://[^/]+).*|\1|')

HOST="${1:?usage: scripts/deploy.sh user@host [aarch64|armv7]}"
ARCH="${2:-aarch64}"
case "$ARCH" in
  aarch64|arm64) TARGET=aarch64-unknown-linux-gnu ;;
  armv7|arm32)   TARGET=armv7-unknown-linux-gnueabihf ;;
  *) echo "unknown arch: $ARCH"; exit 1 ;;
esac
BIN="dist/$TARGET/spanish-learning"
[ -x "$BIN" ] || { echo "no binary at $BIN - run scripts/build-pi.sh $ARCH first"; exit 1; }

REMOTE_DIR=/opt/spanish-learning
STAGE="/tmp/spanish-learning-deploy"
# nginx: the snippets in deploy/ are installed to /etc/nginx/snippets and included
# (once) from these server blocks. Set NGINX_TLS_VHOST="" to skip nginx entirely.
NGINX_TLS_VHOST="${NGINX_TLS_VHOST-/etc/nginx/sites-enabled/vaultwarden}"
NGINX_HTTP_VHOST="${NGINX_HTTP_VHOST-/etc/nginx/sites-enabled/php-apps}"

echo "==> staging files on $HOST"
SSH "$HOST" "mkdir -p $STAGE/data"
RSYNC -az --delete "$BIN" "$HOST:$STAGE/spanish-learning"
RSYNC -az --delete --exclude test-courses/ data/ "$HOST:$STAGE/data/"
# The unit and the nginx snippets carry @PI_PUBLIC_URL@ / @PI_PUBLIC_ORIGIN@
# placeholders; fill them in from deploy.env before copying.
RENDER=$(mktemp -d)
for f in spanish-learning.service nginx-spanish.conf nginx-spanish-http.conf; do
  sed -e "s|@PI_PUBLIC_URL@|$PI_PUBLIC_URL|g" -e "s|@PI_PUBLIC_ORIGIN@|$PI_PUBLIC_ORIGIN|g" "deploy/$f" > "$RENDER/$f"
done
RSYNC -az "$RENDER/spanish-learning.service" "$RENDER/nginx-spanish.conf" "$RENDER/nginx-spanish-http.conf" "$HOST:$STAGE/"
rm -rf "$RENDER"
# App settings that live outside the repo: the public URL. Written to /etc/spanish-learning/env (root, 0600).
ENVFILE=$(mktemp)
{
  echo "# written by scripts/deploy.sh from deploy/deploy.env"
  echo "SKRONGLE_PUBLIC_URL=$PI_PUBLIC_URL"
  echo "SKRONGLE_BASE_PATH=$PI_BASE_PATH"
} > "$ENVFILE"
RSYNC -az "$ENVFILE" "$HOST:$STAGE/app.env"
rm -f "$ENVFILE"
WRITE_ENV="${WRITE_ENV:-0}"   # WRITE_ENV=1 rewrites /etc/spanish-learning/env from deploy.env

echo "==> installing"
SSH "$HOST" "NGINX_TLS_VHOST='$NGINX_TLS_VHOST' NGINX_HTTP_VHOST='$NGINX_HTTP_VHOST' WRITE_ENV='$WRITE_ENV' bash -s" <<'REMOTE'
set -euo pipefail
export PATH="$PATH:/usr/sbin:/sbin"
STAGE=/tmp/spanish-learning-deploy
REMOTE_DIR=/opt/spanish-learning
if ! id spanish >/dev/null 2>&1; then
  sudo useradd --system --home-dir /var/lib/spanish-learning --shell /usr/sbin/nologin spanish
fi
sudo mkdir -p "$REMOTE_DIR" /var/lib/spanish-learning
sudo install -m 0755 "$STAGE/spanish-learning" "$REMOTE_DIR/spanish-learning.new"
sudo mv -f "$REMOTE_DIR/spanish-learning.new" "$REMOTE_DIR/spanish-learning"   # atomic swap
sudo rsync -a --delete "$STAGE/data/" "$REMOTE_DIR/data/"
sudo chown -R root:spanish "$REMOTE_DIR"
sudo chown -R spanish:spanish /var/lib/spanish-learning
sudo chmod 0750 /var/lib/spanish-learning
# Only replace the env file when asked (WRITE_ENV=1); a hand-written
# /etc/spanish-learning/env from before stays as it is otherwise.
if [ "$WRITE_ENV" = 1 ] || [ ! -f /etc/spanish-learning/env ]; then
  sudo mkdir -p /etc/spanish-learning
  sudo install -m 0600 -o root -g root "$STAGE/app.env" /etc/spanish-learning/env
fi
if ! cmp -s "$STAGE/spanish-learning.service" /etc/systemd/system/spanish-learning.service; then
  sudo install -m 0644 "$STAGE/spanish-learning.service" /etc/systemd/system/spanish-learning.service
  sudo systemctl daemon-reload
fi
sudo systemctl enable --now spanish-learning >/dev/null
sudo systemctl restart spanish-learning
sleep 1
sudo systemctl --no-pager --lines=5 status spanish-learning || true

# ---- nginx: install snippets, include them once per vhost, reload ----
install_include() {  # $1 = vhost file, $2 = snippet path, $3 = staged snippet
  local vhost="$1" snippet="$2" src="$3"
  [ -n "$vhost" ] && [ -f "$vhost" ] || { echo "nginx: skipping $vhost (not found)"; return 0; }
  sudo install -m 0644 "$src" "$snippet"
  if ! sudo grep -qF "include $snippet;" "$vhost"; then
    # Insert the include before the first unindented "}" = end of the first server block
    # (location blocks inside it are indented). Keep a one-time backup of the original.
    sudo grep -qE '^}[[:space:]]*$' "$vhost" || { echo "nginx: no server-block end found in $vhost, not touching it"; return 0; }
    # Backup must live OUTSIDE sites-enabled, or nginx would load it as a second site.
    local bak="/var/backups/spanish-learning/$(basename "$vhost").pre-spanish.bak"
    sudo mkdir -p /var/backups/spanish-learning
    [ -f "$bak" ] || sudo cp -p "$vhost" "$bak"
    sudo awk -v inc="    include $snippet; # managed by spanish-learning deploy" '
      !done && /^}[[:space:]]*$/ { print inc; done=1 }
      { print }' "$vhost" | sudo tee "$vhost.tmp" >/dev/null
    sudo mv "$vhost.tmp" "$vhost"
    echo "nginx: added include to $vhost (backup: $bak)"
  fi
}
if [ -n "$NGINX_TLS_VHOST" ]; then
  sudo mkdir -p /etc/nginx/snippets
  install_include "$NGINX_TLS_VHOST"  /etc/nginx/snippets/spanish-learning.conf      "$STAGE/nginx-spanish.conf"
  install_include "$NGINX_HTTP_VHOST" /etc/nginx/snippets/spanish-learning-http.conf "$STAGE/nginx-spanish-http.conf"
  if sudo nginx -t 2>&1 | tail -1; then
    sudo systemctl reload nginx && echo "nginx: reloaded"
  else
    echo "nginx: config test FAILED, not reloading"; exit 1
  fi
fi
rm -rf "$STAGE"
REMOTE

echo "==> done."
