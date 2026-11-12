# Connect an AI assistant

An AI assistant such as Claude Code, Claude or ChatGPT can make and change
your courses for you. You say "make me a three-lesson course about the
planets", it writes the lessons, and Skrongle checks them and puts the
course on your list. Skrongle itself makes no AI call for this and uses none
of your AI allowance: the assistant does the writing, on its own account.

This page is for you if you use Skrongle. The technical details are in the
[MCP server reference](api).

## What an assistant can and cannot do

It can, for your account only:

- read the course format, so it writes lessons Skrongle accepts;
- list your own courses and read their files;
- check a course without saving it;
- make a new course, and change the files of a course you made.

It cannot see your progress, your notes or your answers. It cannot see
anyone else's courses, or the built-in courses or ones shared with you. It
cannot share or delete a course, and it cannot change accounts or settings.
Every course it makes is a course of yours: you can open it, edit it, share
it or delete it like any other.

## First, the owner turns it on

Connecting an app is off until the owner of the server turns it on.

1. Sign in as the owner and open **Manage Users** (the account menu, top
   right).
2. In the **Connected apps** card, click **Let apps connect** so that it
   reads **On**.

The owner sees every connected app of every account in the same card, and can
revoke any of them there. Turning the switch **Off** again stops every app at
once; turning it **On** brings them back.

Who may connect an app: an account with the **Create courses** permission.
In a classroom, a Student account has no permissions, so a Student cannot
connect an app and does not see **Connected apps** on the profile. A Teacher
can. The owner sees every connection, so nothing is hidden from the person
who runs the class. See [Accounts and AI keys](self-hosting#accounts-and-ai-keys)
for permissions.

## What a token is

Most ways of connecting use a **token**. A token is a password for making
courses as you. Whoever holds it can read and change your courses without
signing in.

- Skrongle shows it once, when you make it. It starts with `skr_`.
- Don't paste it into a chat, a screenshot, an e-mail or a shared file.
- Give it only to the one app it is for.
- If you are in doubt where it went, open your profile, find the app under
  **Connected apps** and click **Revoke**. It stops working at once.
- Changing your password also disconnects every app.

Never put a token in a web address. Every example on this page sends it in a
header, where it is not written into logs and browser history.

## Connect from your own computer (a home network)

This works on any install, with nothing set up but the switch above. The
assistant has to run on a computer on the same network as Skrongle (the
same Wi-Fi), so it can reach it. Claude Code is the example; other programs
that run on your computer and take an "MCP server" with a header follow the
same shape.

### Make a token

1. Open **Profile** and find **Connected apps**.
2. Click **Make a token**.
3. Type a **Name** that tells you which app it is for, such as `Claude Code`.
4. Under **What the app may do**, tick only what it needs.
   **Read your courses** is enough if it only needs to look.
   **Create and change your courses** is for making courses.
5. If the assistant only needs one course, tick it under **Only these
   courses**. Tick none for all of your courses. An app limited to some
   courses can change those, but cannot create a new one.
6. Pick **Works for**: **30 days** for a try-out, **90 days** or **A year**
   for something you keep using.
7. Type **Your password**, then click **Make**.
8. **Your new token** appears. Click **Copy**, then **OK**. It will not be
   shown again.

An account keeps at most 4 connected apps. The list shows each app's name,
what it may do, when it expires and when it was last used.

Choose the least that does the job: one course instead of all, read-only if
it only looks, 30 days for a try-out. You can always make another token.

### Add it to Claude Code

Claude Code takes a server by address, with the token as a header. In a terminal, on the
computer you use, with the address you use for Skrongle and your token
(these are made-up examples):

```sh
claude mcp add --transport http skrongle http://skrongle.local:7576/mcp \
  --header "Authorization: Bearer skr_EXAMPLE0000000000000000000000000000000000000000000000000000000000000000"
```

The address ends in `/mcp`. If Skrongle sits at a path such as
`https://example.org/skrongle`, it is `https://example.org/skrongle/mcp`.

Then check it:

```sh
claude mcp list
```

`skrongle` should be listed as connected. Inside Claude Code, `/mcp` shows
the same.

To make the connection for one project only, run the command in that
project's folder (the default). `--scope user` makes it for every project;
use that only if you want it everywhere. To take it away:

```sh
claude mcp remove skrongle
```

Then ask: *Make a three-lesson course about the planets on Skrongle.* The
assistant reads the course format first, writes the lessons, and gives you
a link. Open it, or find the course on **Courses**.

Revoke the token on your profile when you are done with it.

### Other programs on your computer

A program that supports MCP servers over HTTP asks for two things: the
server address (`http://skrongle.local:7576/mcp`) and a header named
`Authorization` with the value `Bearer ` and then your token. If it offers
only a field for a token or an API key, check that it sends it as a Bearer
header. If a program insists on putting the token in the address, don't use
it with Skrongle: it will not work, and the token would be written to logs.

### What plain HTTP means

On a home network Skrongle's address starts with `http://`, not
`https://`. That is fine at home, but it means nothing scrambles what is
sent: anyone who can read traffic on that network can copy the token as it
goes by. At home that is people already on your Wi-Fi. So:

- Don't use a token over public Wi-Fi, or through a router port forwarded to
  Skrongle, unless the address starts with `https://`.
- Don't set Skrongle up to be reached from the internet just so a token
  works. If you want that, read the next section first.

If a token might have been seen, revoke it.

## Connect through a domain (claude.ai and ChatGPT)

The Claude and ChatGPT websites and apps do not run on your computer. They
connect from their own servers on the internet, so they can only reach a
Skrongle that is on the internet. They also do not take a token you hand
them: they sign in, and you click **Allow** on a Skrongle page. That needs
three things to be true first. The server's owner does them once.

1. **The address reaches the box from outside.** A name such as
   `learn.example.org` that opens Skrongle from a phone on mobile data.
2. **A real certificate.** The address starts with `https://`, through a TLS
   proxy such as nginx or Caddy. See "Reaching it from outside" in
   [Self-hosting](self-hosting#reaching-it-from-outside).
3. **`SKRONGLE_PUBLIC_URL` is set to that address**, for example
   `https://learn.example.org`. Without it Skrongle does not offer sign-in
   to apps at all. If Skrongle is at a path such as `/skrongle`, also see
   [Connected apps behind a path](self-hosting#connected-apps-behind-a-path)
   for two lines for the proxy.

Also keep **Let apps connect** on, as above.

**A server on the internet is a different kind of server.** If you forward a
router port, or use a tunnel, to make Skrongle reachable from outside,
anyone in the world can then open its sign-in page. Do not do that without
reading [Accounts and AI keys](self-hosting#accounts-and-ai-keys) (the
`SKRONGLE_SIGNUP` setting: use `invite` or `closed`, not `open`), turning on
two-factor sign-in for the owner (**Profile**, **Two-factor
authentication**), and [Security: connected apps](security-apps).
An app always goes through Skrongle's own sign-in and consent page, and
there is no setting that skips them. Do not put anything in front of
Skrongle that lets people in without signing in.

If you only want to try an assistant, use the home-network way above.

### What you will see

The assistant sends you to a Skrongle page titled "Connect" and the app's
name, as a question. If you are not signed in, you sign in first (password,
then two-factor if you use it). The page then shows:

- what the app wants to do (**Read your courses**, **Create and change your
  courses**);
- the host your answer will go to;
- **Only these courses**: tick the ones the app may use, or none for all;
- **Works for**: **30 days**, **90 days** or **A year**;
- **Allow** and **Deny**.

Click **Allow** only if you started connecting this app yourself. Choose
the least: one course, and **30 days** for a try-out. **Deny** sends nothing
to the app. The app then appears under **Connected apps** on your profile,
where **Revoke** disconnects it.

### claude.ai

Checked on 12 November 2026 against Claude's help page
[Get started with custom connectors using remote MCP](https://support.claude.com/en/articles/11175166-getting-started-with-custom-connectors-using-remote-mcp).
Claude's menus change, so if a name below is not there, follow that page.

On a Free, Pro or Max plan:

1. Open **Customize**, then **Connectors**.
2. Click **+ Add**, then **Add custom connector**.
3. Type a name, for example `Skrongle`.
4. Type the address of your Skrongle, ending in `/mcp`:
   `https://learn.example.org/mcp`. Click **Continue**.
5. Under **Authentication**, choose **Sign in now**. Leave **Request
   headers** empty: do not paste a token there.
6. If it asks for an **OAuth client**, choose **Register automatically**:
   Skrongle registers each app that connects.
7. Click **Add**, then **Connect**. Claude opens Skrongle's sign-in and
   consent page. Click **Allow**.

On a Team or Enterprise plan the organization's owner adds the connector
(**Organization settings**, then **Connectors**), and each person then
clicks **Connect** on it under **Customize**, **Connectors** and signs in to
their own Skrongle account. To use it in a chat, click **+** at the lower
left of the chat, then **Connectors**, and switch Skrongle on.

Claude connects from Anthropic's servers, so this works only when your
Skrongle can be reached from the internet, as above.

### ChatGPT

Checked on 12 November 2026 against OpenAI's page
[Connect from ChatGPT](https://developers.openai.com/apps-sdk/deploy/connect-chatgpt).
ChatGPT's menus change often, and which plans and workspaces may add a
custom server depends on OpenAI and on your workspace's settings, so follow
that page if what you see is different.

1. Open **ChatGPT Plugins** (chatgpt.com/plugins).
2. Click the plus button, then **Add custom MCP server**.
3. Type a name and a short description.
4. Under **Connection**, choose **Public endpoint** and type the address,
   ending in `/mcp`: `https://learn.example.org/mcp`.
5. Under authentication choose the sign-in option (OAuth), not a pasted
   token. Read the warning, then click **I understand and want to
   continue**.
6. Click **Create as a plugin**. ChatGPT lists the tools it found.
7. Install it, start a new chat, type `@` and pick it. ChatGPT sends you to
   Skrongle to sign in and click **Allow**.

## If it does not work

- **Nothing happens, or "not found" (404).** **Let apps connect** is off, or
  the address is wrong. It must end in `/mcp`.
- **"Unauthorized" (401).** The token is wrong, expired or revoked, or the
  account no longer has **Create courses**. Make a new one.
- **Its message says a course "could not load".** The assistant's lessons
  broke a rule of the [course format](course-format). The message names the
  file and the field; the assistant can fix it, so tell it to try again.
- **The assistant cannot make a new course.** The token is limited to some
  courses, and those cannot create a new one. Make a token for all your
  courses.
- **A cloud app says it cannot sign in.** Check the three things under
  "Connect through a domain", especially that `SKRONGLE_PUBLIC_URL` is the
  exact address you typed.
- **A link the assistant gives you does not open.** Without
  `SKRONGLE_PUBLIC_URL` the link is only a path. Open it on the address you
  use for Skrongle.

More questions: [FAQ](faq). The owner's side of this is in
[Self-hosting](self-hosting#connected-apps).
