#!/bin/sh
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/System/slotservices.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILS=0

fail() {
	echo "FAIL: $*"
	FAILS=$((FAILS + 1))
}

make_fake() {
	path="$1"
	out="$2"
	shift 2
	mkdir -p "$(dirname "$path")"
	{
		echo '#!/bin/sh'
		echo "printf '%s\\n' \"\$*\" >> \"$out\""
		for line in "$@"; do echo "$line"; done
		echo 'exit 0'
	} > "$path"
	chmod 755 "$path"
}

reset() {
	rm -rf "$TMP/run" "$TMP/data" "$TMP/svc" "$TMP/bin" "$TMP/wg" "$TMP"/*.log "$TMP/tun"
	mkdir -p "$TMP/run" "$TMP/data" "$TMP/bin" "$TMP/wg"
	printf 'root:$6$old$hash:19000:0:99999:7:::\nnobody:*:19000:0:99999:7:::\n' > "$TMP/run/shadow"
	make_fake "$TMP/bin/dropbear" "$TMP/dropbear.log" "sleep 30 & echo \$! > \"$TMP/run/dropbear.pid\""
	make_fake "$TMP/bin/mkpasswd" "$TMP/mkpasswd.log" 'printf "\$6\$salt\$%s" "$(cat)"'
	make_fake "$TMP/bin/killall" "$TMP/killall.log"
	# BaseOS's dropbear, when FAKE_BASEOS_SSH says it is up.
	make_fake "$TMP/bin/pidof" "$TMP/pidof.log" '[ "${FAKE_BASEOS_SSH:-}" = yes ] && echo 4242 && exit 0' 'exit 1'
	make_fake "$TMP/bin/ip" "$TMP/ip.log"
	make_fake "$TMP/svc/samba/bin/smbpasswd" "$TMP/smbpasswd.log" "cat > \"$TMP/smbpasswd.in\""
	make_fake "$TMP/svc/samba/bin/smbd" "$TMP/smbd.log"
	# tailscaled stays up and opens its socket; as the CLI it says what it was asked.
	make_fake "$TMP/svc/tailscaled" "$TMP/tailscaled.log" \
		"[ \"\${TS_BE_CLI:-}\" = 1 ] && exit 0" \
		": > \"$TMP/run/tailscale/tailscaled.sock\"" \
		'sleep 30'
	make_fake "$TMP/svc/wireguard-go" "$TMP/wg.log" ": > \"$TMP/wg/wg0.sock\""
}

run() {
	PATH="$TMP/bin:$PATH" \
		AGS_SERVICES="$TMP/svc" AGS_RUN="$TMP/run" AGS_DATA="$TMP/data" \
		AGS_SHADOW="$TMP/run/shadow" AGS_DROPBEAR="$TMP/bin/dropbear" \
		AGS_MKPASSWD="$TMP/bin/mkpasswd" AGS_KILLALL="$TMP/bin/killall" AGS_PIDOF="$TMP/bin/pidof" AGS_IP="$TMP/bin/ip" \
		AGS_TUN="$TMP/tun" AGS_WG_RUN="$TMP/wg" AGS_SERVICES_WAIT_S=2 \
		sh "$SCRIPT" "$@"
}

# SSH.
reset
run ssh on || fail "ssh on failed"
grep -q -- "-r $TMP/data/dropbear/dropbear_ed25519_host_key -B -P $TMP/run/dropbear.pid" \
	"$TMP/dropbear.log" || fail "dropbear was not started with BaseOS's keys"
run ssh on
[ "$(wc -l < "$TMP/dropbear.log")" = 1 ] || fail "a second dropbear was started over the first"
run ssh off
[ -f "$TMP/run/dropbear.pid" ] && fail "ssh off left dropbear running"
run ssh off
grep -q dropbear "$TMP/killall.log" || fail "ssh off with no pidfile did not look for dropbear"
reset
FAKE_BASEOS_SSH=yes run ssh on || fail "ssh on with BaseOS's dropbear up failed"
[ -f "$TMP/dropbear.log" ] && fail "a second dropbear was started beside BaseOS's"

# The password.
reset
echo 'hunter2 two' | run password || fail "password failed"
grep -q '^root:\$6\$salt\$hunter2 two:19000:' "$TMP/run/shadow" || fail "root's hash was not replaced"
grep -q '^nobody:\*:' "$TMP/run/shadow" || fail "another user's line was changed"
grep -q -- '-m sha512 -P 0' "$TMP/mkpasswd.log" || fail "the hash is not sha512 from stdin"
: | run password 2>/dev/null && fail "an empty stdin set a password"

# Samba.
reset
echo 'pw' | run samba on /mnt/card || fail "samba on failed"
grep -q 'path = /mnt/card' "$TMP/run/slotsamba/smb.conf" || fail "the card is not what is shared"
grep -q 'max protocol = SMB2' "$TMP/run/slotsamba/smb.conf" || fail "SMB1 is still offered"
grep -q 'guest ok = no' "$TMP/run/slotsamba/smb.conf" || fail "guests can get in"
grep -q -- "-s -a root" "$TMP/smbpasswd.log" || fail "root was not given a Samba password"
[ "$(cat "$TMP/smbpasswd.in")" = "$(printf 'pw\npw')" ] || fail "smbpasswd was not told the password twice"
grep -q -- "-s $TMP/run/slotsamba/smb.conf -D" "$TMP/smbd.log" || fail "smbd was not started on slot's config"
run samba off
grep -q smbd "$TMP/killall.log" || fail "samba off left the connections' smbds"

# Tailscale.
reset
run tailscale on || fail "tailscale on failed"
grep -q -- "--statedir=$TMP/data/tailscale --socket=$TMP/run/tailscale/tailscaled.sock" \
	"$TMP/tailscaled.log" || fail "tailscaled was not started on slot's state and socket"
grep -q -- "--tun=userspace-networking" "$TMP/tailscaled.log" || fail "with no TUN, tailscaled still wanted one"
run tailscale on
[ "$(grep -c statedir "$TMP/tailscaled.log")" = 1 ] || fail "a second tailscaled was started"
run tailscale cli status --json
grep -q -- "--socket=$TMP/run/tailscale/tailscaled.sock status --json" "$TMP/tailscaled.log" ||
	fail "the CLI did not go to slot's tailscaled"
run tailscale off
[ -f "$TMP/run/slot-tailscaled.pid" ] && fail "tailscale off left tailscaled running"

# WireGuard.
reset
run vpn up 2>/dev/null && fail "a tunnel came up with no TUN device"
reset
# A character device stands in for /dev/net/tun.
ln -s /dev/null "$TMP/tun"
run vpn up || fail "vpn up failed"
grep -q '^wg0$' "$TMP/wg.log" || fail "wireguard-go was not started on wg0"
grep -q 'link del wg0' "$TMP/ip.log" || fail "a stale wg0 was not cleared first"
run vpn down
[ -e "$TMP/wg/wg0.sock" ] && fail "vpn down left the socket"
[ "$(grep -c 'link del wg0' "$TMP/ip.log")" = 2 ] || fail "vpn down left wg0"

run nonsense 2>/dev/null && rc=0 || rc=$?
[ "$rc" = 2 ] || fail "an unknown verb exited $rc, want 2"

pkill -f "$TMP" 2>/dev/null || true
if [ "$FAILS" -gt 0 ]; then
	echo "$FAILS failure(s)"
	exit 1
fi
echo "slotservices: ok"
