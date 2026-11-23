		if To_Install skrongle skrongle
		then
			case $G_HW_ARCH in
				1) local arch='armv6l';;
				2) local arch='armv7l';;
				3) local arch='aarch64';;
				*) local arch='x86_64';;
			esac

			# Download
			local fallback_url="https://github.com/alexbeeken/skrongle-mini/releases/download/v0.4.0/skrongle-0.4.0-$arch.tar.gz"
			Download_Install "$(curl -sSfL 'https://api.github.com/repos/alexbeeken/skrongle-mini/releases/latest' | grep -Po "\"browser_download_url\": *\"\K[^\"]*\/skrongle-[0-9.]*-$arch\.tar\.gz(?=\")")" skrongle

			# Reinstall
			[[ -d '/opt/skrongle' ]] && G_EXEC rm -R /opt/skrongle

			# Install
			G_EXEC mv skrongle /opt/

			# User
			Create_User -d /mnt/dietpi_userdata/skrongle skrongle

			# Data dir
			G_EXEC mkdir -p /mnt/dietpi_userdata/skrongle
			G_EXEC chown -R skrongle:skrongle /mnt/dietpi_userdata/skrongle
			G_EXEC chmod 0750 /mnt/dietpi_userdata/skrongle

			# Config: Generated once
			G_EXEC mkdir -p /etc/skrongle
			if [[ ! -f '/etc/skrongle/env' ]]
			then
				G_EXEC install -g skrongle -m 0640 /dev/null /etc/skrongle/env
				cat << '_EOF_' > /etc/skrongle/env || exit 1
# Skrongle mini configuration: https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md#configuration
# Restart to apply changes: systemctl restart skrongle

# Address people open, for the links the server shows
#SKRONGLE_PUBLIC_URL=http://<hostname>.local:7576

# Address and port to listen on
#SKRONGLE_BIND=0.0.0.0:7576

# Who may make an account: open, invite or closed
#SKRONGLE_SIGNUP=invite

# Log level: error, warn, info, debug or trace
#SKRONGLE_LOG=info
_EOF_
			fi

			# Service: https://github.com/alexbeeken/skrongle-mini/blob/main/deploy/dietpi-install.sh
			cat << '_EOF_' > /etc/systemd/system/skrongle.service || exit 1
[Unit]
Description=Skrongle mini (DietPi)
Wants=network-online.target
After=network-online.target remote-fs.target

[Service]
Type=simple
User=skrongle
Group=skrongle
WorkingDirectory=/opt/skrongle
ExecStart=/opt/skrongle/skrongle
Restart=on-failure
RestartPreventExitStatus=78
RestartSec=3

Environment=SKRONGLE_BIND=0.0.0.0:7576
Environment=SKRONGLE_DATA_DIR=/opt/skrongle/data
Environment=SKRONGLE_DB=/mnt/dietpi_userdata/skrongle/learn.db
Environment=SKRONGLE_LOG=info
Environment=SKRONGLE_SIGNUP=invite
EnvironmentFile=-/etc/skrongle/env
Environment=MALLOC_ARENA_MAX=2
Environment=MALLOC_TRIM_THRESHOLD_=65536

MemoryMax=64M
MemoryHigh=48M
TasksMax=32
LimitNOFILE=256
CPUWeight=80
Nice=2

NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/mnt/dietpi_userdata/skrongle
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6
RestrictNamespaces=true
LockPersonality=true
SystemCallArchitectures=native

[Install]
WantedBy=multi-user.target
_EOF_
		fi
