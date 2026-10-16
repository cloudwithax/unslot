#!/bin/sh
# Fetching for slot's updater: GitHub's listings of releases, and the files they point to. A
# file is checked against the checksum its release publishes before it is moved to where BaseOS
# looks for it at boot.
#
#   slotupdate.sh get URL                 print what is at URL
#   slotupdate.sh fetch URL SHA256 DEST   download URL to DEST.part, check it, then move it to DEST
set -u

CURL="${AGS_CURL:-curl}"

case "${1:-}" in
get)
	$CURL -fsSL --connect-timeout 15 --max-time 30 -H 'Accept: application/vnd.github+json' "${2:?a URL}"
	got=$?
	# curl says 22 when the server answered with an error, as GitHub does for no release.
	case "$got" in
	0) ;;
	22) echo "there is no release yet" >&2; exit 1 ;;
	*) echo "GitHub could not be reached" >&2; exit 1 ;;
	esac
	;;
fetch)
	url="${2:?a URL}"
	sha="${3:?a checksum}"
	dest="${4:?where to put it}"
	/bin/rm -f "$dest.part"
	if ! $CURL -fsSL --connect-timeout 15 --retry 2 -o "$dest.part" "$url"; then
		/bin/rm -f "$dest.part"
		echo "the download failed" >&2
		exit 1
	fi
	got="$(sha256sum "$dest.part" | cut -d' ' -f1)"
	if [ "$got" != "$sha" ]; then
		/bin/rm -f "$dest.part"
		echo "the download did not match its checksum" >&2
		exit 1
	fi
	/bin/mv -f "$dest.part" "$dest" && sync
	;;
*)
	echo "usage: slotupdate.sh get URL | fetch URL SHA256 DEST" >&2
	exit 2
	;;
esac
