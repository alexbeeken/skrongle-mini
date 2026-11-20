#!/bin/sh
# Turn an installed Skrongle on a Pi into a classroom box that needs no router
# and no internet (docs/setup-classroom.md). Run it after the install of
# docs/setup-home-wifi.md (steps 1 to 6), while the Pi still has internet:
#
#   sudo sh classroom-install.sh --name "Room 12" --password "a long phrase" --country US
#   sudo sh classroom-install.sh --join          # the Pi joins a separate access point
#   sudo sh classroom-install.sh --undo          # put the home Wi-Fi setup back
#
# Two setups, one app. The app is the same binary with the same settings
# either way; this only changes the network around it.
#
# Setup A (the default): the Pi is the Wi-Fi.
#
#  1. Installs hostapd, dnsmasq and iw if they are missing (the one step that
#     needs the internet). Their own services are left off; Skrongle's units
#     below run them with Skrongle's own settings.
#  2. Gives wlan0 two fixed addresses: 192.168.4.1 (Skrongle) and 192.168.4.2
#     (the redirect, below). skrongle-ap-address.service.
#  3. hostapd: the Wi-Fi, named and locked as asked (WPA2 with a password).
#  4. dnsmasq: hands out addresses and names. skrongle.lan is 192.168.4.1;
#     every other name is 192.168.4.2.
#  5. The redirect: a systemd socket on 192.168.4.2 port 80. Whatever a phone
#     asks for at some other name (its "is there internet?" check, a web
#     page) is answered with a redirect to http://skrongle.lan/login. That
#     is the captive-portal page. A few lines of sh per connection; nothing
#     is installed for it. It is a separate address so Skrongle sees each
#     student's own address, not one shared by a proxy (its rate limits are
#     per address).
#  6. Skrongle: a drop-in for the unit (skrongle.service.d/classroom.conf)
#     that listens on 192.168.4.1 port 80 (it may use a port below 1024, and
#     nothing else) and no longer waits for network-online.target, and
#     SKRONGLE_PUBLIC_URL=http://skrongle.lan in the env file.
#  7. Takes wlan0 out of /etc/network/interfaces (the old file is kept as
#     interfaces.before-classroom), so the Pi stops looking for the home
#     Wi-Fi. This takes effect at the next start, not now: it never cuts
#     the connection you are working over. Then it says to reboot.
#
# Setup B (--join): a separate access point with no internet, which the Pi
# joins (set it up as the guide's step 3 does, with that access point's name).
# The script gives the Pi the name skrongle.local (hostname, Avahi) and has
# Skrongle listen on port 80. The access point hands out addresses; reserve
# one for the Pi there. No captive-portal redirect: only the name works.
#
# --dry-run DIR writes every file under DIR and runs nothing else (no root,
# no packages, no systemctl). scripts/classroom-check.sh uses it.
set -eu

NAME='' PASS='' COUNTRY='' JOIN='' UNDO='' DRY=''
usage() {
  echo "usage: sudo sh $0 --name <wifi name> --password <8 to 63 characters> --country <XX>"
  echo "       sudo sh $0 --join"
  echo "       sudo sh $0 --undo"
  exit "$1"
}
while [ $# -gt 0 ]; do
  case "$1" in
    --name) [ $# -ge 2 ] || usage 2; NAME=$2; shift 2 ;;
    --password) [ $# -ge 2 ] || usage 2; PASS=$2; shift 2 ;;
    --country) [ $# -ge 2 ] || usage 2; COUNTRY=$2; shift 2 ;;
    --join) JOIN=1; shift ;;
    --undo) UNDO=1; shift ;;
    --dry-run) [ $# -ge 2 ] || usage 2; DRY=$2; shift 2 ;;
    -h|--help) usage 0 ;;
    *) usage 2 ;;
  esac
done
if [ -n "$UNDO" ] && { [ -n "$JOIN" ] || [ -n "$NAME$PASS$COUNTRY" ]; }; then usage 2; fi
if [ -n "$JOIN" ] && [ -n "$NAME$PASS$COUNTRY" ]; then usage 2; fi

R=$DRY # prefix of every path
IFACE=wlan0
AP_IP=192.168.4.1
PORTAL_IP=192.168.4.2
AP_NAME=skrongle.lan
JOIN_NAME=skrongle.local
CONF=$R/etc/skrongle
ENVF=$CONF/env
UNITDIR=$R/etc/systemd/system
DROPIN=$UNITDIR/skrongle.service.d/classroom.conf
IFACES=$R/etc/network/interfaces
LIB=$R/usr/local/lib/skrongle
UNITS="skrongle-ap-address.service skrongle-hostapd.service skrongle-dnsmasq.service skrongle-portal.socket skrongle-portal@.service"
MARK="# set by classroom-install.sh"

# Real work only when not a dry run.
run() { [ -n "$DRY" ] || "$@"; }
note() { echo "$*"; }

if [ -z "$DRY" ]; then
  [ "$(id -u)" = 0 ] || { echo "run it as root: sudo sh $0 $*"; exit 1; }
  command -v systemctl >/dev/null 2>&1 || { echo "needs systemd (the DietPi image has it)"; exit 1; }
else
  mkdir -p "$R/etc/skrongle" "$R/etc/network" "$R/etc/systemd/system" "$R/etc/hostapd" "$R/usr/local/lib/skrongle"
  [ -e "$ENVF" ] || : > "$ENVF"
fi

# ---------- undo ----------

if [ -n "$UNDO" ]; then
  for u in skrongle-portal.socket skrongle-hostapd.service skrongle-dnsmasq.service skrongle-ap-address.service; do
    run systemctl disable --now "$u" 2>/dev/null || true
  done
  for u in $UNITS; do rm -f "$UNITDIR/$u"; done
  rm -f "$DROPIN" "$CONF/classroom-dnsmasq.conf" "$CONF/classroom-hostapd.conf" "$LIB/portal-reply.sh"
  rmdir "$UNITDIR/skrongle.service.d" 2>/dev/null || true
  if [ -f "$IFACES.before-classroom" ]; then
    cat "$IFACES.before-classroom" > "$IFACES"
    rm -f "$IFACES.before-classroom"
    note "put back $IFACES: the Pi looks for its home Wi-Fi again"
  fi
  if [ -f "$ENVF" ]; then
    awk -v m="$MARK" 'skip { skip = 0; next } $0 == m { skip = 1; next } { print }' "$ENVF" > "$ENVF.tmp"
    # the home Wi-Fi guide's address
    echo "SKRONGLE_PUBLIC_URL=http://$(hostname).local:7576" >> "$ENVF.tmp"
    cat "$ENVF.tmp" > "$ENVF"; rm -f "$ENVF.tmp"
  fi
  run systemctl daemon-reload
  note "The classroom setup is removed. Reboot to join your home Wi-Fi again: sudo reboot"
  exit 0
fi

# ---------- check what we were given ----------

if [ -z "$DRY" ]; then
  [ -x /opt/skrongle/skrongle ] && [ -f /etc/systemd/system/skrongle.service ] \
    || { echo "Skrongle is not installed yet: do steps 1 to 6 of docs/setup-home-wifi.md first"; exit 1; }
fi

if [ -z "$JOIN" ]; then
  if [ -z "$DRY" ]; then
    [ -d "/sys/class/net/$IFACE" ] || { echo "no $IFACE on this machine: a Pi with Wi-Fi is needed, or use --join"; exit 1; }
    if systemctl is-active --quiet NetworkManager 2>/dev/null; then
      echo "this machine uses NetworkManager for its Wi-Fi; this script is for DietPi (docs/setup-classroom.md)."
      echo "Use a separate access point and --join instead."
      exit 1
    fi
  fi
  # Anything not given as a flag is asked for, when there is a person to ask.
  ask() { # prompt silent -> ANS
    [ -t 0 ] || { echo "a name, a password and a country are needed: see --help"; exit 2; }
    printf '%s' "$1" >&2
    [ -z "$2" ] || stty -echo
    IFS= read -r ANS
    [ -z "$2" ] || { stty echo; echo >&2; }
  }
  if [ -z "$NAME" ]; then ask "Wi-Fi name (what the students will see): " ""; NAME=$ANS; fi
  if [ -z "$PASS" ]; then ask "Wi-Fi password (8 to 63 characters): " 1; PASS=$ANS; fi
  if [ -z "$COUNTRY" ]; then ask "Country code, two letters (US, GB, DE, CA ...): " ""; COUNTRY=$ANS; fi
  # printable ASCII only: hostapd reads the file as written
  case "$NAME$PASS" in *[![:print:]]*) echo "the name and password may use letters, digits, spaces and punctuation only"; exit 1 ;; esac
  [ "${#NAME}" -ge 1 ] && [ "${#NAME}" -le 32 ] || { echo "the Wi-Fi name must be 1 to 32 characters"; exit 1; }
  [ "${#PASS}" -ge 8 ] && [ "${#PASS}" -le 63 ] || { echo "the password must be 8 to 63 characters"; exit 1; }
  case "$COUNTRY" in [A-Za-z][A-Za-z]) COUNTRY=$(echo "$COUNTRY" | tr '[:lower:]' '[:upper:]') ;; *) echo "the country is two letters, such as US"; exit 1 ;; esac
fi

# ---------- packages (needs the internet, once) ----------

need=
if [ -z "$JOIN" ]; then
  for p in hostapd dnsmasq iw; do command -v "$p" >/dev/null 2>&1 || [ -x "/usr/sbin/$p" ] || need="$need $p"; done
else
  command -v avahi-daemon >/dev/null 2>&1 || [ -x /usr/sbin/avahi-daemon ] || need=avahi-daemon
fi
if [ -n "$need" ] && [ -z "$DRY" ]; then
  note "installing$need (this is the one step that needs the internet)"
  # shellcheck disable=SC2086 # $need is a list of package names
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends $need \
    || { echo "could not install$need: is the Pi online? Nothing else was changed."; exit 1; }
fi

# ---------- the app: the same unit, another address ----------

mkdir -p "$UNITDIR/skrongle.service.d" "$LIB" "$CONF"
if [ -z "$JOIN" ]; then
  APP_BIND="$AP_IP:80"; URL="http://$AP_NAME"
  REQ="Requires=skrongle-ap-address.service
After=skrongle-ap-address.service"
else
  APP_BIND="0.0.0.0:80"; URL="http://$JOIN_NAME"; REQ=
fi
cat > "$DROPIN" <<EOF
# Written by deploy/classroom-install.sh (docs/setup-classroom.md).
# The classroom box does not wait for a network: no network-online.target.
[Unit]
Wants=
After=
$REQ

[Service]
# Port 80 is below 1024; this is the only power the app gets for it.
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE
Environment=SKRONGLE_BIND=$APP_BIND
EOF
note "wrote $DROPIN (Skrongle on $APP_BIND)"

# SKRONGLE_PUBLIC_URL in the env file: ours replaces the home guide's.
{
  # (a second run first drops what the first one wrote)
  [ ! -f "$ENVF" ] || awk -v m="$MARK" 'skip { skip = 0; next } $0 == m { skip = 1; next } !/^[ \t]*SKRONGLE_PUBLIC_URL=/ { print }' "$ENVF"
  echo "$MARK"
  echo "SKRONGLE_PUBLIC_URL=$URL"
} > "$ENVF.tmp"
# keep the file's owner and mode: write into it, do not replace it
cat "$ENVF.tmp" > "$ENVF"; rm -f "$ENVF.tmp"
note "set SKRONGLE_PUBLIC_URL=$URL in $ENVF"

# ---------- setup B: a name, nothing else ----------

if [ -n "$JOIN" ]; then
  run hostnamectl set-hostname skrongle
  if [ -z "$DRY" ] && [ -f /etc/hosts ]; then
    if grep -q '^127\.0\.1\.1' /etc/hosts; then
      sed -i 's/^127\.0\.1\.1.*/127.0.1.1 skrongle/' /etc/hosts
    else
      echo "127.0.1.1 skrongle" >> /etc/hosts
    fi
  fi
  run systemctl daemon-reload
  run systemctl enable avahi-daemon
  run systemctl restart avahi-daemon
  run systemctl restart skrongle
  note
  note "Done. On the access point's network the box is $URL"
  note "In the access point's settings, give this Pi a fixed address (a DHCP reservation)."
  exit 0
fi

# ---------- setup A: the Wi-Fi ----------

umask 077
cat > "$CONF/classroom-hostapd.conf" <<EOF
interface=$IFACE
driver=nl80211
ssid=$NAME
country_code=$COUNTRY
ieee80211d=1
hw_mode=g
channel=6
ieee80211n=1
wmm_enabled=1
auth_algs=1
wpa=2
wpa_key_mgmt=WPA-PSK
rsn_pairwise=CCMP
wpa_passphrase=$PASS
EOF
umask 022

cat > "$CONF/classroom-dnsmasq.conf" <<EOF
# Written by deploy/classroom-install.sh. No internet: no upstream servers.
interface=$IFACE
listen-address=$AP_IP
bind-interfaces
no-resolv
no-hosts
dhcp-authoritative
dhcp-range=192.168.4.10,192.168.4.250,255.255.255.0,12h
dhcp-option=option:router,$AP_IP
dhcp-option=option:dns-server,$AP_IP
dhcp-leasefile=/var/lib/misc/skrongle.leases
# Skrongle's name opens Skrongle; every other name opens the redirect.
address=/$AP_NAME/$AP_IP
address=/#/$PORTAL_IP
EOF

cat > "$LIB/portal-reply.sh" <<'EOF'
#!/bin/sh
# One connection to the redirect address: read the request's head, answer with
# a redirect to Skrongle's sign-in page.
CR=$(printf '\r')
while IFS= read -r line; do
  case "$line" in ''|"$CR") break ;; esac
done
printf 'HTTP/1.1 302 Found\r\nLocation: http://skrongle.lan/login\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n'
EOF
chmod 0755 "$LIB/portal-reply.sh"

cat > "$UNITDIR/skrongle-ap-address.service" <<EOF
[Unit]
Description=Skrongle classroom box: the fixed addresses on $IFACE
After=sys-subsystem-net-devices-$IFACE.device
Requires=sys-subsystem-net-devices-$IFACE.device

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/bin/env ip addr replace $AP_IP/24 dev $IFACE
ExecStart=/usr/bin/env ip addr replace $PORTAL_IP/24 dev $IFACE
ExecStart=/usr/bin/env ip link set $IFACE up
ExecStop=/usr/bin/env ip addr flush dev $IFACE

[Install]
WantedBy=multi-user.target
EOF

cat > "$UNITDIR/skrongle-hostapd.service" <<EOF
[Unit]
Description=Skrongle classroom box: the Wi-Fi
Requires=skrongle-ap-address.service
After=skrongle-ap-address.service

[Service]
ExecStart=/usr/sbin/hostapd $CONF/classroom-hostapd.conf
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

cat > "$UNITDIR/skrongle-dnsmasq.service" <<EOF
[Unit]
Description=Skrongle classroom box: addresses and names
Requires=skrongle-ap-address.service
After=skrongle-ap-address.service skrongle-hostapd.service

[Service]
ExecStart=/usr/sbin/dnsmasq --keep-in-foreground --conf-file=$CONF/classroom-dnsmasq.conf
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

cat > "$UNITDIR/skrongle-portal.socket" <<EOF
[Unit]
Description=Skrongle classroom box: redirect any other name to the sign-in page

[Socket]
# FreeBind: it may open before the address exists.
ListenStream=$PORTAL_IP:80
FreeBind=yes
Accept=yes
MaxConnections=64

[Install]
WantedBy=sockets.target
EOF

cat > "$UNITDIR/skrongle-portal@.service" <<EOF
[Unit]
Description=Skrongle classroom box: one redirect

[Service]
ExecStart=/usr/local/lib/skrongle/portal-reply.sh
StandardInput=socket
StandardOutput=socket
RuntimeMaxSec=5
DynamicUser=yes
NoNewPrivileges=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=true
EOF
note "wrote the Wi-Fi, address, name and redirect units"

# wlan0 out of ifupdown (DietPi), effective at the next start.
if [ -f "$IFACES" ] && [ ! -f "$IFACES.before-classroom" ] && grep -qE "^(allow-hotplug|auto|iface)[[:space:]]+$IFACE([[:space:]]|\$)" "$IFACES"; then
  cp -p "$IFACES" "$IFACES.before-classroom"
  awk -v i="$IFACE" '
    /^[^ \t#]/ { c = ($0 ~ "^(allow-hotplug|auto|iface)[ \t]+" i "([ \t]|$)") }
    c && !/^#/ && NF { print "#skrongle# " $0; next }
    { print }' "$IFACES.before-classroom" > "$IFACES"
  note "took $IFACE out of $IFACES (the old file is $IFACES.before-classroom)"
fi

run systemctl daemon-reload
# The distro's own services stay off: Skrongle's units run them.
for s in hostapd dnsmasq; do
  run systemctl disable --now "$s" 2>/dev/null || true
done
run systemctl mask hostapd 2>/dev/null || true
run systemctl enable skrongle-ap-address.service skrongle-hostapd.service skrongle-dnsmasq.service skrongle-portal.socket

note
note "Done. Reboot now: sudo reboot"
note "The Pi then leaves your home Wi-Fi and becomes this one:"
note
note "  Wi-Fi name:  $NAME"
note "  Password:    $PASS"
note "  Address:     $URL   (or http://$AP_IP)"
note
note "To go back to the home Wi-Fi: sudo sh $0 --undo, then sudo reboot."
