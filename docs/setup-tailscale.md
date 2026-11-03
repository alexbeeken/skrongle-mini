# Away from home with Tailscale

[Set up on home Wi-Fi](setup-home-wifi) leaves Skrongle reachable only on
your own network. This guide adds access from anywhere, for the people you
choose, without opening your home network to the internet. It uses
Tailscale, a free (for personal use) private network: every device you add
can reach every other, wherever it is, and nobody else can.

It is written for a person and for an AI assistant alike; the commands are
exact. Do the home Wi-Fi guide first. The technical reference is
[docs/self-hosting.md](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md).

## How this stays safe

- Your router still forwards nothing. Tailscale connects out from the Pi
  and out from each phone; the internet never gets a way in.
- Only devices signed in to your Tailscale account (or ones you explicitly
  share with) can reach the Pi.
- Traffic is encrypted end to end, and Skrongle gets a real HTTPS address
  with a certificate, so browsers treat it as a proper site.
- Never use Tailscale's **Funnel** feature with Skrongle. Funnel publishes a
  service to the whole internet, which is exactly what this guide avoids.

## Step 1: Make a Tailscale account

Go to [tailscale.com](https://tailscale.com) and sign up (it signs in with
an existing Google, Microsoft, Apple or GitHub account). Install the
Tailscale app on the phone or laptop you are using now and sign in to it.

In the admin console at [login.tailscale.com/admin/dns](https://login.tailscale.com/admin/dns),
turn on **MagicDNS** and **HTTPS Certificates**. These give the Pi a name
and let it serve HTTPS.

## Step 2: Put Tailscale on the Pi

From your computer, sign in to the Pi (`ssh skrongle`) and run:

```sh
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
```

`tailscale up` prints a link. Open it on your computer, sign in, and approve
the Pi. Back on the Pi, the command finishes. Check:

```sh
tailscale status
```

Your Pi is listed, with a name like `skrongle` and an address starting with
`100.`. In the admin console, under **Machines**, click the Pi's `…` menu
and choose **Disable key expiry**, so it never silently drops off the
network.

## Step 3: Serve Skrongle over HTTPS inside Tailscale

Tailscale can put an HTTPS front on a local port, with a valid certificate,
visible only inside your Tailscale network:

```sh
sudo tailscale serve --bg 8080
```

It prints the address, of the form `https://skrongle.<your-tailnet>.ts.net`.
Write it down. Then tell Skrongle its new address and that it is behind
HTTPS now:

```sh
sudo tee /etc/skrongle/env > /dev/null <<EOF
SKRONGLE_PUBLIC_URL=https://skrongle.<your-tailnet>.ts.net
SKRONGLE_SECURE_COOKIES=1
EOF
sudo systemctl restart skrongle
```

(Put your real address on the first line.) Open that address on your phone:
Skrongle, over HTTPS, from anywhere the phone has Tailscale on.

The old `http://skrongle.local:8080` keeps working at home. If you'd rather
the Pi answered only through Tailscale, add
`SKRONGLE_BIND=127.0.0.1:8080` to the same file and restart; then nothing on
the home network reaches it directly, only Tailscale's HTTPS front.

## Step 4: Let the others in

Each person needs the Tailscale app on their phone or laptop, and needs to
be on your Tailscale network. Two ways:

- **Same account.** For a family, the simplest: install Tailscale on their
  devices and sign in with your account. Tailscale's personal plan allows
  several users and plenty of devices for free.
- **Their own account.** Share the Pi with them: in the admin console,
  the Pi's `…` menu → **Share…**, which makes an invitation link. They
  accept it with their own Tailscale account and see only the Pi, nothing
  else of yours.

Then send each of them a Skrongle invitation link from **Manage Users**, as
at home.

## Step 5: Sign in to the Pi itself through Tailscale (optional)

If you want to administer the Pi from away, the SSH key from the home guide
already works over Tailscale: add a second entry to `~/.ssh/config` on your
computer, pointing at the Tailscale name instead of the local one:

```
Host skrongle-away
  HostName skrongle.<your-tailnet>.ts.net
  User dietpi
  IdentityFile ~/.ssh/skrongle-pi
  IdentitiesOnly yes
```

Then `ssh skrongle-away`. Password sign-in stayed off, so the key is the
only way in, from anywhere.

## Keep it that way

- Keep Tailscale updated along with the rest of the Pi:
  `sudo dietpi-update` updates the system, and Tailscale updates itself
  through the Pi's package manager.
- If a phone is lost, remove it in the admin console under **Machines**; it
  loses access at once.
- `tailscale serve status` shows what the Pi is serving; `sudo tailscale
  serve reset` turns the HTTPS front off again if you ever need to.
- Never run `tailscale funnel`. If you see "Funnel" in any instruction,
  it is not for this.
