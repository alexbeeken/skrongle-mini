#!/bin/sh
# What can be checked of deploy/classroom-install.sh (#229) with no Pi and
# no root: it runs the script with --dry-run into a temporary folder and
# checks the files it writes, the redirect program on a request, and --undo.
# Needs only sh and awk; also runs shellcheck, dnsmasq --test and
# systemd-analyze verify where they are installed. CI runs it in the
# "cargo test" job. One line per check, ok or FAIL; non-zero on any FAIL.
#
# What it cannot check, and #231 does on a real Pi Zero 2 W: that the radio
# starts as an access point, how many devices join it, that a phone opens
# the sign-in page by itself.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/../deploy/classroom-install.sh"
FAILED=0
ok() { echo "  ok   $1"; }
fail() { echo "  FAIL $1"; FAILED=1; }
check() { if "$@" >/dev/null 2>&1; then ok "$DESC"; else fail "$DESC"; fi; }
has() { grep -q -- "$2" "$1"; }

W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
mkroot() { mkdir -p "$1/etc/network" "$1/etc/skrongle"
  printf 'auto lo\niface lo inet loopback\n\nallow-hotplug wlan0\niface wlan0 inet dhcp\n   wpa-conf /etc/wpa_supplicant/wpa_supplicant.conf\n\nallow-hotplug eth0\niface eth0 inet dhcp\n' > "$1/etc/network/interfaces"
  printf '#SKRONGLE_BIND=127.0.0.1:7576\nSKRONGLE_PUBLIC_URL=http://skrongle.local:7576\n' > "$1/etc/skrongle/env"; }

DESC="the script is plain sh"; check sh -n "$SCRIPT"
if command -v shellcheck >/dev/null 2>&1; then DESC="shellcheck is clean"; check shellcheck -s sh "$SCRIPT"; fi

# --- setup A: the Pi is the Wi-Fi ---
A="$W/a"; mkroot "$A"
OUT=$(sh "$SCRIPT" --dry-run "$A" --name 'Room 12' --password 'pa ss#word$1' --country us 2>&1) && ok "setup A runs" || { echo "$OUT"; fail "setup A runs"; }
H="$A/etc/skrongle/classroom-hostapd.conf"
DESC="hostapd: name, WPA2 password, upper-case country"
has "$H" '^ssid=Room 12$' && has "$H" '^wpa_passphrase=pa ss#word\$1$' && has "$H" '^wpa=2$' && has "$H" '^country_code=US$' && ok "$DESC" || fail "$DESC"
[ "$(stat -c %a "$H" 2>/dev/null || stat -f %Lp "$H")" = 600 ] && ok "the hostapd file (it holds the password) is mode 0600" || fail "the hostapd file is mode 0600"
D="$A/etc/skrongle/classroom-dnsmasq.conf"
DESC="dnsmasq: skrongle.lan is the box, every other name the redirect, no upstream DNS"
has "$D" '^address=/skrongle.lan/192.168.4.1$' && has "$D" '^address=/#/192.168.4.2$' && has "$D" '^no-resolv$' && ok "$DESC" || fail "$DESC"
P="$A/etc/systemd/system/skrongle.service.d/classroom.conf"
DESC="the app: port 80 on its own address, no wait for the network"
has "$P" '^Environment=SKRONGLE_BIND=192.168.4.1:80$' && has "$P" '^Wants=$' && has "$P" '^After=$' && has "$P" '^AmbientCapabilities=CAP_NET_BIND_SERVICE$' && ok "$DESC" || fail "$DESC"
E="$A/etc/skrongle/env"
DESC="env: one SKRONGLE_PUBLIC_URL, http://skrongle.lan, other lines kept"
[ "$(grep -c '^SKRONGLE_PUBLIC_URL=' "$E")" = 1 ] && has "$E" '^SKRONGLE_PUBLIC_URL=http://skrongle.lan$' && has "$E" '^#SKRONGLE_BIND=' && ok "$DESC" || fail "$DESC"
sh "$SCRIPT" --dry-run "$A" --name 'Room 12' --password 'pa ss#word$1' --country US >/dev/null 2>&1
DESC="a second run keeps one SKRONGLE_PUBLIC_URL"; [ "$(grep -c '^SKRONGLE_PUBLIC_URL=' "$E")" = 1 ] && ok "$DESC" || fail "$DESC"
I="$A/etc/network/interfaces"
DESC="wlan0 is out of /etc/network/interfaces, eth0 and lo are kept, the old file is saved"
! grep -q '^[^#].*wlan0' "$I" && ! grep -q '^ *wpa-conf' "$I" && has "$I" '^allow-hotplug eth0$' && has "$I" '^iface lo inet loopback$' && [ -f "$I.before-classroom" ] && ok "$DESC" || fail "$DESC"

# the redirect: any request gets a 302 to the sign-in page
R="$A/usr/local/lib/skrongle/portal-reply.sh"
for REQ in 'GET /generate_204 HTTP/1.1\r\nHost: connectivitycheck.gstatic.com\r\n\r\n' 'GET /hotspot-detect.html HTTP/1.0\r\nHost: captive.apple.com\r\nUser-Agent: x\r\n\r\n' 'POST /any HTTP/1.1\r\nHost: example.org\r\nContent-Length: 3\r\n\r\nabc' 'GET / HTTP/1.1\r\n'; do
  REPLY=$(printf "$REQ" | sh "$R" | tr -d '\r')
  DESC="the redirect answers $(printf "$REQ" | head -n1 | tr -d '\r')"
  case "$REPLY" in "HTTP/1.1 302 Found"*"Location: http://skrongle.lan/login"*) ok "$DESC" ;; *) fail "$DESC"; echo "$REPLY" ;; esac
done

if command -v dnsmasq >/dev/null 2>&1; then DESC="dnsmasq --test accepts its config"; check dnsmasq --test -C "$D"; fi
if command -v systemd-analyze >/dev/null 2>&1; then
  DESC="systemd-analyze verify accepts the units"
  # the binaries do not exist on a laptop; only the syntax is asked about
  if systemd-analyze verify "$A"/etc/systemd/system/skrongle-*.service "$A"/etc/systemd/system/skrongle-portal.socket 2>&1 | grep -v 'Command .* is not executable\|Failed to prepare' | grep -qi 'unknown\|invalid\|bad'; then fail "$DESC"; else ok "$DESC"; fi
fi

# --- the way back with no ssh (fallback) ---
FB="$A/usr/local/lib/skrongle/classroom-fallback.sh"
DESC="setup A leaves a copy of itself, the fallback, and the timer and card units"
[ -x "$FB" ] && [ -f "$A/usr/local/lib/skrongle/classroom-install.sh" ] && has "$A/etc/systemd/system/skrongle-wifi-fallback.timer" '^OnBootSec=5min$' \
  && has "$A/etc/systemd/system/skrongle-card-undo.service" 'classroom-fallback.sh card' && ok "$DESC" || fail "$DESC"
# stand-ins: systemctl (is-active answers by $W/hostapd-up, reboot is logged) and the undo
cat > "$W/systemctl" <<EOF
#!/bin/sh
case "\$1" in
  is-active) [ -e "$W/hostapd-up" ] ;;
  reboot) echo reboot >> "$W/calls" ;;
esac
EOF
cat > "$W/undo" <<EOF
#!/bin/sh
echo undo >> "$W/calls"
[ ! -e "$W/undo-fails" ]
EOF
chmod +x "$W/systemctl" "$W/undo"
fb() { STATE="$W/state" SYSTEMCTL="$W/systemctl" UNDO_CMD="$W/undo" CARD_DIRS="$W/card $W/card2" sh "$FB" "$@" >/dev/null 2>&1; }
calls() { cat "$W/calls" 2>/dev/null | tr '\n' ' '; }
: > "$W/hostapd-up"; rm -f "$W/calls"; fb late
DESC="fallback: the Wi-Fi is up: nothing happens"; [ -z "$(calls)" ] && ok "$DESC" || fail "$DESC"
rm -f "$W/hostapd-up"; fb late
DESC="fallback: the Wi-Fi is not up: undo, then one reboot, flag left"; [ "$(calls)" = "undo reboot " ] && [ -e "$W/state/fallback-done" ] && ok "$DESC" || fail "$DESC ($(calls))"
fb late; fb late
DESC="fallback: it cannot loop (a second and third run do nothing)"; [ "$(calls)" = "undo reboot " ] && ok "$DESC" || fail "$DESC ($(calls))"
rm -rf "$W/state" "$W/calls"; : > "$W/undo-fails"; fb late
DESC="fallback: if undo fails, it does not reboot"; [ "$(calls)" = "undo " ] && ok "$DESC" || fail "$DESC ($(calls))"
rm -rf "$W/state" "$W/calls" "$W/undo-fails"; mkdir -p "$W/card"; : > "$W/card/skrongle-undo"; : > "$W/hostapd-up"; fb card
DESC="fallback: skrongle-undo on the card: undo, reboot, the file is removed"; [ "$(calls)" = "undo reboot " ] && [ ! -e "$W/card/skrongle-undo" ] && ok "$DESC" || fail "$DESC ($(calls))"
rm -f "$W/calls"; fb card
DESC="fallback: no file on the card: nothing happens"; [ -z "$(calls)" ] && ok "$DESC" || fail "$DESC"
DESC="setup A records what --undo must undo (hostapd mask)"; [ -e "$A/var/lib/skrongle-classroom/masked-hostapd" ] && ok "$DESC" || fail "$DESC"

# --join on top of setup A is refused, nothing changes
DESC="--join after setup A is refused with the command to run"
if OUT=$(sh "$SCRIPT" --dry-run "$A" --join 2>&1); then fail "$DESC"; else case "$OUT" in *"--undo"*) ok "$DESC" ;; *) fail "$DESC" ;; esac; fi

# --- undo puts it back ---
sh "$SCRIPT" --dry-run "$A" --undo >/dev/null 2>&1
DESC="--undo removes the units, the drop-in and the configs"
[ ! -e "$P" ] && [ ! -e "$H" ] && [ ! -e "$D" ] && [ ! -e "$R" ] && [ -z "$(ls "$A"/etc/systemd/system 2>/dev/null)" ] && ok "$DESC" || fail "$DESC"
DESC="--undo puts back /etc/network/interfaces"; has "$I" '^allow-hotplug wlan0$' && [ ! -e "$I.before-classroom" ] && ok "$DESC" || fail "$DESC"
DESC="--undo removes the classroom address from the env file"; ! has "$E" 'skrongle.lan' && ! has "$E" 'classroom-install' && [ "$(grep -c '^SKRONGLE_PUBLIC_URL=' "$E")" = 1 ] && ok "$DESC" || fail "$DESC"

DESC="--undo unmasks hostapd (the record is gone) and removes the fallback files"
[ ! -e "$A/var/lib/skrongle-classroom/masked-hostapd" ] && [ ! -e "$FB" ] && ok "$DESC" || fail "$DESC"

# --undo with the saved interfaces file missing: says so, exits non-zero
M="$W/m"; mkroot "$M"
sh "$SCRIPT" --dry-run "$M" --name x --password longenough --country US >/dev/null 2>&1
rm -f "$M/etc/network/interfaces.before-classroom"
DESC="--undo says plainly, and fails, when the saved interfaces file is missing"
if OUT=$(sh "$SCRIPT" --dry-run "$M" --undo 2>&1); then fail "$DESC"; else case "$OUT" in *PROBLEM*"before-classroom"*) ok "$DESC" ;; *) fail "$DESC" ;; esac; fi

# --- wlan0 managed some other way: stop, change nothing ---
X="$W/x"; mkroot "$X"; mkdir -p "$X/etc/network/interfaces.d"
printf 'allow-hotplug wlan0\niface wlan0 inet dhcp\n' > "$X/etc/network/interfaces.d/wifi"
cp "$X/etc/network/interfaces" "$W/x-before"
DESC="wlan0 set up in interfaces.d: refused, nothing changed"
if OUT=$(sh "$SCRIPT" --dry-run "$X" --name x --password longenough --country US 2>&1); then fail "$DESC"; else
  [ -z "$(ls -A "$X/etc/systemd/system")" ] && cmp -s "$X/etc/network/interfaces" "$W/x-before" && case "$OUT" in *"interfaces.d/wifi"*"--join"*) ok "$DESC" ;; *) fail "$DESC" ;; esac; fi
Y="$W/y"; mkroot "$Y"
printf '\nallow-hotplug eth0\niface eth0 inet dhcp\n   post-up ip link set wlan0 up\n' >> "$Y/etc/network/interfaces"
cp "$Y/etc/network/interfaces" "$W/y-before"
DESC="wlan0 still mentioned after the change: refused, the file put back"
if OUT=$(sh "$SCRIPT" --dry-run "$Y" --name x --password longenough --country US 2>&1); then fail "$DESC"; else
  cmp -s "$Y/etc/network/interfaces" "$W/y-before" && [ ! -e "$Y/etc/network/interfaces.before-classroom" ] && [ -z "$(ls -A "$Y/etc/systemd/system")" ] && ok "$DESC" || fail "$DESC"; fi

# --- bad input is refused, nothing is written ---
Z="$W/z"; mkroot "$Z"
DESC="a non-ASCII password is refused"; ! sh "$SCRIPT" --dry-run "$Z" --name x --password 'pässword12' --country US >/dev/null 2>&1 && ok "$DESC" || fail "$DESC"
DESC="a non-ASCII Wi-Fi name is refused"; ! sh "$SCRIPT" --dry-run "$Z" --name 'Klasse ä' --password longenough --country US >/dev/null 2>&1 && ok "$DESC" || fail "$DESC"
DESC="a 64-byte password is refused, a 63-byte one is taken"
P64=$(printf 'a%.0s' $(seq 1 64)); P63=$(printf 'a%.0s' $(seq 1 63))
! sh "$SCRIPT" --dry-run "$Z" --name x --password "$P64" --country US >/dev/null 2>&1 && sh "$SCRIPT" --dry-run "$Z" --name x --password "$P63" --country US >/dev/null 2>&1 && ok "$DESC" || fail "$DESC"
if command -v bash >/dev/null 2>&1; then
  DESC="the same under bash: 32 two-byte letters (64 bytes) are refused"
  ! bash "$SCRIPT" --dry-run "$Z" --name x --password "$(printf 'ä%.0s' $(seq 1 32))" --country US >/dev/null 2>&1 && ok "$DESC" || fail "$DESC"
fi
B="$W/b"; mkroot "$B"
DESC="a password under 8 characters is refused"; ! sh "$SCRIPT" --dry-run "$B" --name x --password short --country US >/dev/null 2>&1 && ok "$DESC" || fail "$DESC"
DESC="a country that is not two letters is refused"; ! sh "$SCRIPT" --dry-run "$B" --name x --password longenough --country USA >/dev/null 2>&1 && ok "$DESC" || fail "$DESC"
DESC="--join with a Wi-Fi name is refused"; ! sh "$SCRIPT" --dry-run "$B" --join --name x >/dev/null 2>&1 && ok "$DESC" || fail "$DESC"
DESC="nothing was written by the refused runs"; [ -z "$(ls -A "$B/etc/systemd/system")" ] && [ ! -e "$B/etc/skrongle/classroom-hostapd.conf" ] && ok "$DESC" || fail "$DESC"

# --- setup B: a separate access point ---
J="$W/j"; mkroot "$J"
sh "$SCRIPT" --dry-run "$J" --join >/dev/null 2>&1 && ok "setup B runs" || fail "setup B runs"
DESC="setup B: port 80 on every address, skrongle.local, no Wi-Fi of its own, wlan0 untouched"
has "$J/etc/systemd/system/skrongle.service.d/classroom.conf" '^Environment=SKRONGLE_BIND=0.0.0.0:80$' && has "$J/etc/skrongle/env" '^SKRONGLE_PUBLIC_URL=http://skrongle.local$' \
  && [ ! -e "$J/etc/skrongle/classroom-hostapd.conf" ] && [ ! -e "$J/etc/systemd/system/skrongle-hostapd.service" ] && ! [ -e "$J/etc/network/interfaces.before-classroom" ] && ok "$DESC" || fail "$DESC"
DESC="setup B records the Pi's name; --undo takes it back and succeeds"
[ -f "$J/var/lib/skrongle-classroom/hostname-before" ] && sh "$SCRIPT" --dry-run "$J" --undo >/dev/null 2>&1 </dev/null && [ ! -e "$J/var/lib/skrongle-classroom/hostname-before" ] && [ ! -e "$J/etc/systemd/system/skrongle.service.d/classroom.conf" ] && ok "$DESC" || fail "$DESC"

[ "$FAILED" = 0 ] && echo "classroom check: ok" || echo "classroom check: FAILED"
exit "$FAILED"
