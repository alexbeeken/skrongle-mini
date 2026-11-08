# Set up Skrongle on a new Raspberry Pi (home Wi-Fi only)

This guide takes a Raspberry Pi Zero 2 W out of its box and ends with
Skrongle running on it, reachable from every phone and laptop on your home
Wi-Fi, and from nowhere else. It assumes nothing: if you can copy and paste,
you can do this. Every step says what to type and what you should see.

This is the one path from "I have a Pi" to "my family can use it and
nobody else can". Follow it top to bottom. At the end you have:

- Skrongle running on the Pi, reachable only from your home Wi-Fi;
- your own account, the owner's, with two-factor sign-in on;
- invitation links for the people you choose;
- a backup on your computer.

It is written for a person and for an AI assistant alike. If an assistant
is doing this with you, it can follow the steps as written; the commands
are exact.

Already have a Pi (or any 64-bit Linux machine) running and can sign in to
it? Skip to [step 5](#step-5-install-skrongle). To reach your Skrongle when
you are away from home, do this guide first, then
[Away from home with Tailscale](setup-tailscale). Everything else (every
setting, a proxy, recovery commands) is in the
[Self-hosting reference](self-hosting).

## What you need

| | |
|---|---|
| A Raspberry Pi Zero 2 W | the computer, the size of a stick of gum |
| A microSD card, 16 GB or more | the Pi's whole system lives on it |
| A micro-USB power supply, 5 V 2.5 A | the official one is safest |
| A computer with an SD card slot or a USB card reader | only for the setup |
| Your Wi-Fi name and password | the Pi joins it like a phone does |

The Pi needs no screen, keyboard or mouse at any point. Everything happens
from your computer over Wi-Fi.

## How this stays private

- The Pi joins your home Wi-Fi and listens only there. Your home router does
  not let anyone on the internet reach it, as long as you never "forward" a
  port to it. Nothing in this guide asks you to.
- You sign in to the Pi with a key that lives on your computer, not a
  password. Password sign-in to the Pi is turned off before it ever boots.
- Skrongle itself takes new accounts only through invitation links you make.

## Step 1: Make an SSH key on your computer

An SSH key is a pair of files: a private one that stays on your computer and
a public one you give to the Pi. Make one just for this Pi.

On a Mac or Linux, open Terminal. On Windows, open PowerShell. Type:

```sh
ssh-keygen -t ed25519 -f ~/.ssh/skrongle-pi -C "skrongle pi"
```

It asks for a passphrase: choose one and remember it (it protects the key if
your computer is ever stolen). You now have `~/.ssh/skrongle-pi` (private,
never share it) and `~/.ssh/skrongle-pi.pub` (public). Print the public one;
you'll paste it in a moment:

```sh
cat ~/.ssh/skrongle-pi.pub
```

It is one line starting with `ssh-ed25519`.

Then tell your computer to use this key, and only this key, for the Pi. Add
these lines to `~/.ssh/config` (create the file if it does not exist):

```
Host skrongle
  HostName skrongle.local
  User dietpi
  IdentityFile ~/.ssh/skrongle-pi
  IdentitiesOnly yes
```

From now on `ssh skrongle` reaches the Pi.

## Step 2: Write DietPi to the card

DietPi is a small, fast Linux for the Pi.

1. Download the DietPi image from
   [dietpi.com/#download](https://dietpi.com/#download): the entry
   **Raspberry Pi 2/3/4/Zero 2**, **ARMv8** (one image covers the Zero 2 W).
   It is a `.img.xz` file; leave it compressed.
2. Install [Raspberry Pi Imager](https://www.raspberrypi.com/software/) on
   your computer and open it. Put the microSD card in.
3. **Choose device:** Raspberry Pi Zero 2 W. **Choose OS:** *Use custom*,
   then the DietPi file you downloaded. **Choose storage:** the microSD card.
4. Click **Next**. Imager asks whether to apply OS customisation settings:
   choose **No** (DietPi has its own settings file, next step). Confirm, and
   wait for it to write and verify.

Do not remove the card yet.

## Step 3: Tell the Pi about your Wi-Fi and your key

The card now shows up on your computer as a small drive (called `boot` or
`bootfs`, or just the card's name). Two text files on it set the Pi up before
its first start, so it never needs a screen.

Open `dietpi-wifi.txt` in a plain-text editor (TextEdit in plain-text mode,
Notepad, VS Code) and fill in the first entry:

```
aWIFI_SSID[0]='Your Wi-Fi name'
aWIFI_KEY[0]='your Wi-Fi password'
```

Leave the other lines of that entry as they are (they already say the
usual home Wi-Fi security, WPA).

Open `dietpi.txt` and change these lines (search for each name; keep
everything else as it is). Paste your public key, the `ssh-ed25519 …` line
from step 1, where it says so:

```
AUTO_SETUP_NET_WIFI_ENABLED=1
AUTO_SETUP_NET_WIFI_COUNTRY_CODE=US
AUTO_SETUP_NET_HOSTNAME=skrongle
AUTO_SETUP_SSH_PUBKEY=ssh-ed25519 AAAA… skrongle pi
SOFTWARE_DISABLE_SSH_PASSWORD_LOGINS=1
AUTO_SETUP_AUTOMATED=1
AUTO_SETUP_GLOBAL_PASSWORD=choose-a-long-password-here
AUTO_SETUP_INSTALL_SOFTWARE_ID=152
```

What each does:

| Setting | Why |
|---|---|
| `WIFI_ENABLED=1`, `COUNTRY_CODE` | joins your Wi-Fi; the country code sets the legal radio channels (two letters: `US`, `GB`, `DE`, `CA` …) |
| `HOSTNAME=skrongle` | the Pi's name on your network: `skrongle.local` |
| `SSH_PUBKEY` | your key is installed before the first boot |
| `DISABLE_SSH_PASSWORD_LOGINS=1` | nobody can sign in to the Pi with a password, ever |
| `AUTOMATED=1` | the first boot runs without questions |
| `GLOBAL_PASSWORD` | the password for `sudo` on the Pi; it is not accepted over the network. Choose a long one and write it down |
| `SOFTWARE_ID=152` | installs Avahi, which is what makes `skrongle.local` work from your phone and laptop |

Save both files, eject the card properly, and put it in the Pi.

## Step 4: First boot

Plug the power in. The green light flickers while it starts. The first boot
takes 5 to 10 minutes: it grows the system to fill the card, joins your
Wi-Fi, updates itself and installs Avahi. There is nothing to do but wait.

After ten minutes, from your computer:

```sh
ssh skrongle
```

The first time, it asks whether to trust the Pi's fingerprint: answer `yes`.
Then it asks for your key's passphrase. You should land at a prompt that
looks like `dietpi@skrongle:~$`. You are on the Pi.

If it says it cannot find `skrongle.local`, wait a few more minutes and try
again. If it still fails, look in your router's list of connected devices for
one called `skrongle`, and use its address instead of `skrongle.local` in
`~/.ssh/config`.

## Step 5: Install Skrongle

Still on the Pi (the `dietpi@skrongle:~$` prompt). Paste the whole block;
the first line finds the newest release by itself:

```sh
V=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/alexbeeken/skrongle-mini/releases/latest); V=${V##*/v}; echo "installing $V"
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz.sha256
sha256sum -c skrongle-$V-$(uname -m).tar.gz.sha256
sudo mkdir -p /opt/skrongle /etc/skrongle
sudo tar -xzf skrongle-$V-$(uname -m).tar.gz -C /opt/skrongle
echo "SKRONGLE_PUBLIC_URL=http://$(hostname).local:8080" | sudo tee /etc/skrongle/env
sudo cp /opt/skrongle/skrongle.service /etc/systemd/system/
sudo systemctl enable --now skrongle
```

The first line prints `installing` and a version number such as `0.1.2`.
The `sha256sum` line must print `OK`: it proves the download is the real
release; if it doesn't, stop here. `sudo` asks for the password you chose
in step 3.

## Step 6: Check it

```sh
sudo /opt/skrongle/skrongle doctor --service
```

It prints one screen about the install. No line should start with
`PROBLEM:`; if one does, it says what to fix. Then, on your phone or laptop
(on the same Wi-Fi), open **http://skrongle.local:8080**. You should see
Skrongle's sign-in page.

If the page doesn't open, use the Pi's address instead: `hostname -I` on
the Pi prints it first (something like `192.168.1.23`), and the page is
`http://192.168.1.23:8080`.

## Step 7: Make your account, the owner's

There is no account yet, and the register page only says this server has no
owner yet. That is on purpose: nobody else on your Wi-Fi can make the first
account before you. The first account is made through the **owner link**, which the server
wrote to its log when it started. Print it on the Pi:

```sh
sudo journalctl -u skrongle -g "Owner link" -o cat | tail -n 1
```

It prints one line ending in a link like
`http://skrongle.local:8080/register?invite=…`. Open that link on your phone
or laptop and register. You are the owner: your account menu (the picture,
top right) has **Manage Users**.

## Step 8: Turn on two-factor sign-in

In your account menu, open **Profile → Two-factor authentication** and
follow it with an authenticator app on your phone (Google Authenticator,
1Password, Aegis, …). From then on, signing in needs the password and a
code from the phone. Ask everyone you invite to do the same.

## Step 9: Invite your people

On **Manage Users**, click **Make an invitation link**, copy it (it is shown
only once) and send it to one person however you like. Each link works once,
for 7 days; make one per person. Skrongle holds up to 8 accounts.

Optional, on the same page under **AI features**: paste an API key from one
of the AI providers to turn on the tutor and course generation for everyone.
One key serves the whole box, billed to you.

## Step 10: Make a backup

Everything Skrongle knows is two files in `/var/lib/skrongle`: the database
`learn.db` and its key `learn.db.key`. Make your first copy now. On the Pi,
pack the two together (Skrongle stops for a second so the copy is
consistent):

```sh
sudo systemctl stop skrongle
(umask 077 && rm -f ~/skrongle-backup.tar.gz && sudo tar -czf - -C /var/lib/skrongle . > ~/skrongle-backup.tar.gz)
sudo systemctl start skrongle
```

Then copy it to your **computer**: type `exit` to leave the Pi, and on your
computer run

```sh
scp -O skrongle:skrongle-backup.tar.gz ~/
ls -l ~/skrongle-backup.tar.gz
```

(`-O` is needed because the Pi's small SSH server speaks the older copy
protocol.) `ls` should show the file, at least a few hundred KB. The backup holds every
account and the key to the saved AI keys: keep it private, and always keep
the two files together. Do this again every month or so, and before any big
change. Restoring is in the reference,
[Backup and restore](self-hosting#backup-and-restore).

## Keep it that way

- **Never forward a port** on your router to the Pi. The guide for reaching
  it from outside is [Away from home with Tailscale](setup-tailscale); it
  needs no port forwarding either.
- **Update** every month or two. Skrongle: `sudo /opt/skrongle/update.sh`
  (it keeps a copy of the database from before the update). The Pi's
  system: `sudo dietpi-update`.
- **If something is off**, run `sudo /opt/skrongle/skrongle doctor --service`
  on the Pi: one screen of facts to paste into a question.

## If you get stuck

- The [self-hosting FAQ](self-hosting-faq) answers the first questions
  people ask.
- Ask in [GitHub Discussions](https://github.com/alexbeeken/skrongle-mini/discussions),
  with the output of `skrongle doctor`.
- The [Self-hosting reference](self-hosting) has every setting and
  command, a proxy in front, and what to do if you lock yourself out.
