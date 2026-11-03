# Set up Skrongle on a new Raspberry Pi (home Wi-Fi only)

This guide takes a Raspberry Pi Zero 2 W out of its box and ends with
Skrongle running on it, reachable from every phone and laptop on your home
Wi-Fi, and from nowhere else. It assumes nothing: if you can copy and paste,
you can do this. Every step says what to type and what you should see.

It is written for a person and for an AI assistant alike. If an assistant
is doing this with you, it can follow the steps as written; the commands
are exact.

The technical reference, for experts and for AI, is
[docs/self-hosting.md](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md).
To reach your Skrongle when you are away from home, do this guide first,
then [Away from home with Tailscale](setup-tailscale).

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

Still on the Pi (the `dietpi@skrongle:~$` prompt). Pick the newest version
from the [releases page](https://github.com/alexbeeken/skrongle-mini/releases)
and put its number on the first line, then paste the whole block:

```sh
V=0.1.0
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-aarch64.tar.gz
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-aarch64.tar.gz.sha256
sha256sum -c skrongle-$V-aarch64.tar.gz.sha256
sudo mkdir -p /opt/skrongle /etc/skrongle
sudo tar -xzf skrongle-$V-aarch64.tar.gz -C /opt/skrongle
echo "SKRONGLE_PUBLIC_URL=http://skrongle.local:8080" | sudo tee /etc/skrongle/env
sudo cp /opt/skrongle/skrongle.service /etc/systemd/system/
sudo systemctl enable --now skrongle
```

The `sha256sum` line must print `OK`: it proves the download is the real
release. `sudo` asks for the password you chose in step 3.

Check that it runs:

```sh
systemctl status skrongle --no-pager
```

You should see `active (running)`. Skrongle is now at
**http://skrongle.local:8080** for every device on your Wi-Fi.

## Step 6: Make your account, the owner's

Skrongle takes new accounts only through links. The very first account is
made through the **owner link**, which the server wrote to its log when it
started. Print it:

```sh
sudo journalctl -u skrongle -g "Owner link" -o cat | tail -n 1
```

Open that link on your phone or laptop, make your account, and you are the
owner: your account menu has **Manage Users**. From there:

- **Invite** everyone else: each invitation is a link to send however you
  like; it works once.
- **AI** (optional): paste an API key from one of the AI providers to turn on
  the tutor and course generation for everyone. One key serves the whole
  box.
- Turn on **two-factor sign-in** with an authenticator app on your own
  profile, and ask the others to as well.

## Step 7: Keep it that way

- **Never forward a port** on your router to the Pi. The guide for reaching
  it from outside is [Away from home with Tailscale](setup-tailscale); it
  needs no port forwarding either.
- **Updates.** Skrongle: `sudo /opt/skrongle/update.sh`. The Pi's system:
  `sudo dietpi-update`. Do both every month or two.
- **Backups.** Everything Skrongle knows is two files. Copy them to your
  computer now and then:

  ```sh
  scp -O skrongle:/var/lib/skrongle/learn.db ~/skrongle-backup.db
  scp -O skrongle:/var/lib/skrongle/learn.db.key ~/skrongle-backup.db.key
  ```

  (`-O` is needed because the Pi's small SSH server speaks the older copy
  protocol.) The `.key` file unlocks the AI keys stored in the database;
  keep the two together, and keep them private.
- **If something is off**, `skrongle doctor` on the Pi prints one screen of
  facts to paste into a question:

  ```sh
  sudo SKRONGLE_DB=/var/lib/skrongle/learn.db /opt/skrongle/skrongle doctor
  ```

## If you get stuck

- The [self-hosting FAQ](self-hosting-faq) answers the first questions
  people ask.
- Ask in [GitHub Discussions](https://github.com/alexbeeken/skrongle-mini/discussions),
  with the output of `skrongle doctor`.
- The full technical reference, every setting and command, is
  [docs/self-hosting.md](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md).
