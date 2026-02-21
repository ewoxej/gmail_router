# Gmail Router

Automatic email router for Gmail that filters and deletes messages based on configuration. The program filters by the "to" address, so it makes sense if you own your own domain and route mail to it.
Personally, I run a Docker container on my own server, but you can build the project yourself. Either way, obtaining Google credentials for API access is a required step.

## Gmail API Setup

### 1. Creating a Project and Enabling Gmail API

Go to Google Cloud Console
Create a new project or select an existing one
Navigate to "APIs & Services" → "Library"
Find "Gmail API" and click "Enable"

### 2. Creating OAuth2 Credentials

Navigate to "APIs & Services" → "Credentials"
Click "Create Credentials" → "OAuth client ID"
Select "Desktop app" as the application type
Enter a name (e.g., "Gmail Router") and click "Create"
Download the JSON credentials file and save it as secret.json in the configuration folder

The folder path depends on your OS:
Linux: `~/.config/gmail_router`
MacOS: `/Users/username/Library/Application Support/gmail_router`
Windows: `C:\Users\username\AppData\Roaming\gmail_router`

### 3. Setting up scopes

1. Go to "APIs & Services" → "Data Access"
2. Add scope: `https://mail.google.com/` (Grants full permissions to delete, send emails, etc.)

## Installation and running

### With docker compose:

```yml
services:
  gmail_router:
    image: gmail_router
    container_name: gmail_router
    volumes:
      - config/gmail_router:/root/.config/gmail_router
    restart: unless-stopped
```

### Manual building:

```bash
git clone https://github.com/ewoxej/gmail_router.git
cd gmail_router
cargo build --release
```

### Configuration and Launch

Copy the sample configuration files and edit them:

On first launch:
1. A browser will open for Google authorization.
2. Allow access for the application.
3. The program will scan all emails and create a routing.yaml file.
4. All found addresses will be added with the true (allowed) flag.

The last scan date will also be recorded in routing.yaml. The next scan will check only new emails, not all emails.
The program will run continuously, checking email every check_interval_seconds.
Block the desired addresses by setting the value to false in routing.yaml.

## License

MIT
