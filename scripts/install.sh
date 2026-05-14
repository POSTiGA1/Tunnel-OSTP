#!/bin/bash
set -e

# Official repository settings
GITHUB_REPO="ospab/ostp"
INSTALL_DIR="/opt/ostp"

echo "========================================================"
echo " Installing Ospab Stealth Transport Protocol (OSTP)"
echo "========================================================"

# Verify superuser privileges
if [ "$EUID" -ne 0 ]; then
  echo "[Error] This script must be run with root privileges (sudo)."
  exit 1
fi

# Create target directory
mkdir -p "$INSTALL_DIR"

# ---------------------------------------------------------
# System Architecture Detection
# ---------------------------------------------------------
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) ARCH="amd64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    i386|i686) ARCH="386" ;;
    armv7l) ARCH="armv7" ;;
    *) 
        echo "[Warning] Unknown architecture $ARCH, falling back to amd64."
        ARCH="amd64"
        ;;
esac

# Fetch execution binary
echo "Fetching the latest stable version from the repository..."
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST_RELEASE" ] || [[ "$LATEST_RELEASE" == *"null"* ]]; then
   echo "[Notice] Failed to automatically retrieve release tag for ${GITHUB_REPO}."
   echo "Enter a direct URL to the compiled .tar.gz archive"
   echo "or press Enter if the binary is already in $INSTALL_DIR/ostp."
   read -p "URL: " DIRECT_URL
   if [ -n "$DIRECT_URL" ]; then
      TEMP_TAR="/tmp/ostp_temp.tar.gz"
      curl -L "$DIRECT_URL" -o "$TEMP_TAR"
      tar -xzf "$TEMP_TAR" -C "$INSTALL_DIR" ostp
      rm -f "$TEMP_TAR"
   fi
else
   ARCHIVE_NAME="ostp-linux-${ARCH}.tar.gz"
   DOWNLOAD_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_RELEASE}/${ARCHIVE_NAME}"
   echo "Downloading archive for linux-$ARCH: $DOWNLOAD_URL ..."
   
   TEMP_TAR="/tmp/ostp_temp.tar.gz"
   # Fetch archive with basic error handling
   HTTP_CODE=$(curl -sL -w "%{http_code}" "$DOWNLOAD_URL" -o "$TEMP_TAR")
   
   if [ "$HTTP_CODE" -eq 200 ]; then
      tar -xzf "$TEMP_TAR" -C "$INSTALL_DIR" ostp
      rm -f "$TEMP_TAR"
   else
      echo "[Error] Failed to download the file (HTTP status $HTTP_CODE)."
      echo "Verify that the version $LATEST_RELEASE is published and fully compiled on GitHub."
      rm -f "$TEMP_TAR"
      exit 1
   fi
fi

if [ -f "$INSTALL_DIR/ostp" ]; then
   chmod +x "$INSTALL_DIR/ostp"
   echo "Executable configured successfully at $INSTALL_DIR/ostp."
else
   echo "[Error] Binary file not found in $INSTALL_DIR/ostp. Aborting setup."
   exit 1
fi

# ---------------------------------------------------------
# Automatic Update Detection (Preserves Settings)
# ---------------------------------------------------------
if [ -f "$INSTALL_DIR/config.json" ]; then
   echo "--------------------------------------------------------"
   echo "[Update] Existing configuration detected at $INSTALL_DIR/config.json."
   echo "[Update] Binary successfully updated to version ${LATEST_RELEASE:-latest}."
   
   if systemctl is-active --quiet ostp.service 2>/dev/null; then
      echo "[Update] Restarting service ostp to apply the new version..."
      systemctl restart ostp.service
      echo "[Update] Service ostp restarted successfully."
   else
      echo "[Update] Service ostp is registered but not currently running."
      echo "Start the service manually to apply changes: systemctl start ostp"
   fi
   echo "--------------------------------------------------------"
   echo "Update completed successfully!"
   exit 0
fi

# Interactive Setup Menu
echo "--------------------------------------------------------"
echo "Select configuration mode:"
echo "1) Configure Server"
echo "2) Configure Client"
echo "--------------------------------------------------------"
read -p "Enter choice [1-2]: " NODE_MODE

cd "$INSTALL_DIR"

if [ "$NODE_MODE" == "1" ]; then
  echo "Initializing server configuration..."
  ./ostp --init server --config config.json
  
  read -p "Enter IP and port to accept incoming traffic [default: 0.0.0.0:50000]: " LISTEN_ADDR
  if [ -n "$LISTEN_ADDR" ]; then
     sed -i "s/\"listen\": \"0.0.0.0:50000\"/\"listen\": \"$LISTEN_ADDR\"/g" config.json
  fi
  
  read -p "How many access keys to generate? [default: 1]: " KEYS_COUNT
  KEYS_COUNT=${KEYS_COUNT:-1}
  
  if [ "$KEYS_COUNT" -gt 1 ]; then
     echo "Generating additional security keys..."
     NEW_KEYS=$(./ostp -g -c "$KEYS_COUNT" | sed 's/^/      "/;s/$/",/' | sed '$ s/,$//')
     sed -i '/"access_keys": \[/,/\]/c\  "access_keys": [\n'"$NEW_KEYS"'\n  ],' config.json
     echo "Successfully generated and wrote $KEYS_COUNT keys."
  fi
  echo "Server configuration completed. Config file: $INSTALL_DIR/config.json"

elif [ "$NODE_MODE" == "2" ]; then
  echo "Initializing client configuration..."
  ./ostp --init client --config config.json
  
  read -p "Enter remote server address (IP:PORT): " REMOTE_SERVER
  if [ -n "$REMOTE_SERVER" ]; then
     sed -i "s/\"server\": \"127.0.0.1:50000\"/\"server\": \"$REMOTE_SERVER\"/g" config.json
  else
     echo "[Warning] No remote address provided, keeping default (127.0.0.1:50000)."
  fi
  
  read -p "Enter access key (leave blank to generate automatically via ostp -g): " ACCESS_KEY
  if [ -z "$ACCESS_KEY" ]; then
     ACCESS_KEY=$(./ostp -g)
     echo "Automatically generated client key: $ACCESS_KEY"
  fi
  sed -i "s/\"access_key\": \"[^\"]*\"/\"access_key\": \"$ACCESS_KEY\"/g" config.json

  read -p "Enter local SOCKS5 listening address [default: 127.0.0.1:1088]: " SOCKS_BIND
  if [ -n "$SOCKS_BIND" ]; then
     sed -i "s/\"socks5_bind\": \"127.0.0.1:1088\"/\"socks5_bind\": \"$SOCKS_BIND\"/g" config.json
  fi
  echo "Client configuration completed. Config file: $INSTALL_DIR/config.json"

else
  echo "[Error] Invalid selection choice."
  exit 1
fi

# Register Systemd daemon
echo "Registering system service..."
cat <<EOF > /etc/systemd/system/ostp.service
[Unit]
Description=Ospab Stealth Transport Protocol Service
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/ostp --config $INSTALL_DIR/config.json
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable ostp.service >/dev/null 2>&1

echo "--------------------------------------------------------"
echo "Installation completed successfully."
echo "Configuration file saved at $INSTALL_DIR/config.json"
echo "Service 'ostp' has been registered but not started."
echo "Start the service manually using: systemctl start ostp"
echo "--------------------------------------------------------"
