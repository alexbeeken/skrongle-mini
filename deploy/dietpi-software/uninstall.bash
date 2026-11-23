		if To_Uninstall skrongle
		then
			Remove_Service skrongle 1 1
			G_EXEC rm -Rf /opt/skrongle /mnt/dietpi_userdata/skrongle /etc/skrongle
		fi
