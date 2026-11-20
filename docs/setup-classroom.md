# Set up a classroom box

A classroom may have no router and no internet. This guide makes a
Raspberry Pi Zero 2 W with Skrongle on it work there anyway. Students join
a Wi-Fi, open one address, and sign in. Nothing leaves the room.

It starts where [Set up on home Wi-Fi](setup-home-wifi) ends: Skrongle
already runs on the Pi. You do that part at home, with internet, once.

## Pick a setup

There are two ways to give the room a Wi-Fi.

| | **A: the Pi is the Wi-Fi** | **B: a small access point, the Pi joins it** |
|---|---|---|
| What you buy | nothing more | a cheap access point or travel router |
| How many devices | **not measured yet** | what the access point says it takes |
| A phone opens the sign-in page by itself | yes, that is what the script sets up | no, students type the address |
| The address | `http://skrongle.lan` | `http://skrongle.local` |

**How many devices the Pi's own Wi-Fi takes has not been measured on a real
Pi Zero 2 W yet.** Its chip is small, and it may not take a full class. Until it is measured, use **B for a full class**. Use A to try it,
or for a handful of devices.

## What works without the internet, and what does not

Everything students do in a lesson works: lessons, quizzes, review, games,
progress, and the courses already on the box. The box does not look for
the internet at start, and nothing waits on it.

Three things need the internet and do not work in the room:

- **Generating a course** with AI.
- **The AI tutor.**
- **Connecting an AI assistant** to the box.

Make courses on another computer, then upload them (see
[Building and generating courses](making-courses)).

The Pi has no clock that keeps time when the power is cut. How the box
should keep the date and time across a power cut is not settled yet.

## What you need

| | |
|---|---|
| The Pi with Skrongle on it, and your account | from the [home Wi-Fi guide](setup-home-wifi), steps 1 to 7 |
| A power supply | and a way to keep the Pi powered all day |
| For B: an access point or travel router | any that makes a Wi-Fi with a password and does not insist on internet to work |

## Step 1: Install Skrongle at home first

Do steps 1 to 7 of [Set up on home Wi-Fi](setup-home-wifi); step 7 makes
your account, the owner's. Steps 8 to 10 (two-factor sign-in, invitations, a
backup) are easiest now too. The Pi needs the internet now, to download
Skrongle and, in A, two small programs for the Wi-Fi. After this step it
never needs it again.

## Step 2, setup A: the Pi becomes the Wi-Fi

Choose the Wi-Fi name and password the students will use. The password is
8 to 63 characters. Also give your country as two letters (`US`, `GB`, `DE`,
`CA` ...): it decides which radio channels are allowed.

On the Pi (`ssh skrongle`, as in the home guide):

```sh
curl -fsSLO https://raw.githubusercontent.com/alexbeeken/skrongle-mini/main/deploy/classroom-install.sh
sudo sh classroom-install.sh --name "Room 12" --password "a long phrase" --country US
```

It installs what it needs, then prints the name, password and address. It
changes nothing about the connection you are working over yet. When it
says so:

```sh
sudo reboot
```

After the reboot the Pi has left your home Wi-Fi and is the room's Wi-Fi.
You can no longer reach it from home. To work on it, join its Wi-Fi and use
`ssh -i ~/.ssh/skrongle-pi dietpi@192.168.4.1`.

What the script set up, in a line each:

- the Wi-Fi, with your name and password;
- the fixed address `192.168.4.1`, and the name `skrongle.lan` for it;
- Skrongle on port 80, so no `:7576` is typed;
- every other web address a phone tries is sent to Skrongle's sign-in page.
  This is what makes a phone open it by itself.

To change the name or password, run the command again. To go back to the
home Wi-Fi: `sudo sh classroom-install.sh --undo`, then `sudo reboot`.

## Step 2, setup B: a separate access point

1. Set up the access point with a Wi-Fi name and password. It needs no
   internet; leave its internet port empty. Switch off anything in it that
   expects internet.
2. Make the Pi join it. On the Pi, run `sudo dietpi-config`, then
   **Network Options: Adapters**, **WiFi**, and enter the access point's
   name and password.
3. Run the script. The Pi is online at home for this one, so do it before
   the Pi leaves:

   ```sh
   curl -fsSLO https://raw.githubusercontent.com/alexbeeken/skrongle-mini/main/deploy/classroom-install.sh
   sudo sh classroom-install.sh --join
   ```

4. In the access point's settings, give the Pi a fixed address (often called
   a reservation or static lease), so it does not change.

The script gives the Pi the name `skrongle.local` and has Skrongle listen on
port 80. It does not touch the Wi-Fi. Not every device understands a
`.local` name; if one does not, give that student the Pi's address from the
access point's list of devices.

## Step 3: Check it

Join the room's Wi-Fi from a phone or computer and open the address from the
board (step 4). You should see Skrongle's sign-in page. Sign in with the
account you made at home.

## Step 4: What to write on the board

```
Wi-Fi:     Room 12
Password:  (the one you chose)
Open:      skrongle.lan     (setup B: skrongle.local)
```

A phone that joins the Wi-Fi may offer to "sign in to the network". Tap it:
that is Skrongle's sign-in page. If nothing pops up, open a browser and type
the address.

## If something is off

- **The Wi-Fi does not show up.** Give the Pi a minute after power. In A,
  check the country code is right for where you are.
- **It shows up but a device cannot join.** Check the password. Then count
  the devices: in A the Pi's Wi-Fi has a limit, which is why a full class
  belongs on B.
- **A phone says there is no internet.** That is true, and fine. Keep the
  connection and open the address.
- **A page opens but sign-in will not stay.** A phone's small pop-up
  browser may not keep it. Open the address in the normal browser instead.
- **The date is wrong after a power cut.** See the note about the clock
  above.
- More in the [Self-hosting FAQ](self-hosting-faq). Every setting is in the
  [Self-hosting reference](self-hosting).

What each step does, and what it writes, is in
[`deploy/classroom-install.sh`](https://github.com/alexbeeken/skrongle-mini/blob/main/deploy/classroom-install.sh),
commented from the top.
