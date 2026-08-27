# Gmail Router

Automatic email router for Gmail that filters and deletes messages based on configuration. The program filters by the "to" address, so it makes sense if you own your own domain and route mail to it.
Personally, I run a Docker container on my own server, but you can build the project yourself. Either way, obtaining Google credentials for API access is a required step.

It ships a small **web UI** (Dioxus fullstack): you sign in with Google in the
browser, then pick a per-address action from a dropdown. The background router
keeps running and applies your choices to incoming mail every
`check_interval_seconds`.

Available actions per address:
- **Keep** — routed through untouched (default).
- **Trash** — move to Trash (recoverable ~30 days).
- **Spam** — mark as spam.
- **Delete** — permanent delete (no Trash).
- **Forward** — send a copy to another address, then move the original to Trash.

## Gmail API Setup

### 1. Creating a Project and Enabling Gmail API

Go to Google Cloud Console
Create a new project or select an existing one
Navigate to "APIs & Services" → "Library"
Find "Gmail API" and click "Enable"

### 2. Creating OAuth2 Credentials

Navigate to "APIs & Services" → "Credentials"
Click "Create Credentials" → "OAuth client ID"
Select **"Web application"** as the application type (the app now signs in
through the browser, not a desktop popup).
Under "Authorized redirect URIs" add `http://localhost:8080/auth/callback`
(and/or your deployed URL, e.g. `https://router.example.com/auth/callback`).
Enter a name (e.g., "Gmail Router") and click "Create".
Download the JSON credentials file and save it as `secret.json` in the
configuration folder.

The folder path depends on your OS:
Linux: `~/.config/gmail_router`
MacOS: `/Users/username/Library/Application Support/gmail_router`
Windows: `C:\Users\username\AppData\Roaming\gmail_router`

If your instance is reachable at a non-default URL, set the `GMAIL_ROUTER_BASE_URL`
environment variable (e.g. `https://router.example.com`) so the redirect URI the
app builds matches the one you registered above.

### 3. Setting up scopes

1. Go to "APIs & Services" → "Data Access"
2. Add scope: `https://mail.google.com/` (Grants full permissions to delete, send emails, etc.)

## Installation and running

### With docker compose:

```yml
services:
  gmail_router:
    image: ghcr.io/ewoxej/gmail_router:latest
    container_name: gmail_router
    volumes:
      - config/gmail_router:/root/.config/gmail_router
    restart: unless-stopped
```

### Manual building:

The app is a [Dioxus](https://dioxuslabs.com) fullstack app, so it's built with
the Dioxus CLI (`dx`), which compiles both the web client and the server binary:

```bash
git clone https://github.com/ewoxej/gmail_router.git
cd gmail_router
cargo install dioxus-cli --version 0.7.3 --locked
dx serve            # dev server at http://localhost:8080
# or, for a production build:
dx bundle --release --platform web
```

### Configuration and Launch

1. Put your Google **Web-application** OAuth client JSON at `secret.json` in the
   config folder (or point `GOOGLE_CLIENT_SECRET` at it).
2. Open the web UI (`http://localhost:8080`).
3. Click **"Sign in with Google"** and grant access. Each user's refresh token
   is stored per-user in the database, so you only sign in once.
4. The router scans your inbox and lists the discovered addresses (all **Keep**
   by default).
5. Pick an action (Trash / Spam / Delete / Forward) for any address to have the
   router apply it to future mail sent there; use **"Scan now"** to pick up
   newly seen recipients.

The background router runs continuously, sweeping every signed-in user every
`CHECK_INTERVAL_SECONDS` and applying their choices to new mail.

## Multi-user & authentication

State lives in a single SQLite database (`gmail_router.db` in the config dir, or
`DATA_DIR`), isolated per user. Each user has their own domain, Google refresh
token, and routing table.

- **Reverse proxy (Authelia/nginx/Traefik):** the proxy authenticates the user
  and forwards a `Remote-User` header. The app trusts it **only** when the
  request also carries `X-Proxy-Auth: <PROXY_AUTH_SECRET>` (the proxy must inject
  this and strip any client-supplied copy). Unknown users are auto-provisioned.
- **API token:** send `Authorization: Bearer <token>` to drive the routing API
  headlessly. Mint/revoke tokens in the UI's **API tokens** panel; only their
  SHA-256 hash is stored.
- **Local dev:** set `AUTH_ENABLED=false` to run as a single fixed user
  (`DEV_USER`, default `dev`) with no proxy.

### Environment variables

| Var | Default | Purpose |
|-----|---------|---------|
| `DATA_DIR` | config dir | Where `gmail_router.db` lives. |
| `GOOGLE_CLIENT_SECRET` | `<config>/secret.json` | Shared OAuth client JSON. |
| `GMAIL_ROUTER_BASE_URL` | `http://localhost:8080` | Base for the OAuth redirect URI (`{base}/auth/callback`). |
| `CHECK_INTERVAL_SECONDS` | `3600` | Router sweep interval. |
| `AUTH_ENABLED` | on | Set `false` to disable auth (local dev). |
| `DEV_USER` | `dev` | The fixed user when `AUTH_ENABLED=false`. |
| `PROXY_AUTH_SECRET` | — | Shared secret gating the `Remote-User` path (fail-closed if unset). |
| `MIGRATE_AS_USER` | `dev` | On first boot, imports any legacy `credentials.yaml` / `routing.yaml` / `authorized_user.json` from the config dir into the DB under this username (one-time, idempotent). |

The old file config (`credentials.yaml`, `routing.yaml`, `authorized_user.json`)
is read **only** for that one-time migration; everything is DB-backed afterward.

## Decision mode (external enforcement)

Set `ROUTER_MODE=decision` to run the app as a **policy store + decision API**
only: it does **not** touch Gmail (no background loop, no Google sign-in
required). Something external — e.g. a **Cloudflare Email Worker** — asks it what
to do with each incoming recipient and enforces the answer.

**`GET /api/route`** (Bearer token required):

| Query | Meaning |
|-------|---------|
| `?address=foo@example.com` | Uses the local-part (`foo`). |
| `?local_part=foo` | The bare local-part. |

Response:

```json
{ "local_part": "foo", "action": "keep", "forward_to": null, "allow": true }
```

`action` is one of `keep` / `trash` / `spam` / `delete` / `forward` (with
`forward_to` set); `allow` is `true` only for `keep`. An **unknown** address is
added to the list as `keep` (default-allow) and reported as such — so the list
grows itself as mail arrives, and you tighten policy in the UI afterward.

Mint a token in the UI's **API tokens** panel. Example Cloudflare Email Worker:

```js
export default {
  async email(message, env) {
    const url = `${env.ROUTER_URL}/api/route?address=${encodeURIComponent(message.to)}`;
    const r = await fetch(url, { headers: { Authorization: `Bearer ${env.ROUTER_TOKEN}` } });
    const { action, forward_to, allow } = await r.json();
    if (allow) return;                                   // keep: deliver normally
    if (action === "forward" && forward_to) return message.forward(forward_to);
    message.setReject("Blocked by gmail_router");        // trash/spam/delete
  },
};
```

## License

MIT
