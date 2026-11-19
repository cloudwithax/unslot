#!/bin/sh
# Which ELF files exceed the A133P SDK's glibc 2.23 baseline, by imported symbol
# versions. This is stricter than the tested Brick Pro firmware's glibc 2.33.
#
#   check-glibc.sh FILE...     fail if any of them does
#   check-glibc.sh -r DIR      list those under DIR, and pass
set -eu
FLOOR=2.23

# The newest GLIBC_ version FILE imports, or nothing for one that is not an ELF.
needs() {
	objdump -T "$1" 2> /dev/null | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -uV |
		tail -n 1
}

over() {
	[ -n "$1" ] && [ "$(printf '%s\n%s\n' "$1" "$FLOOR" | sort -V | tail -n 1)" != "$FLOOR" ]
}

if [ "${1:-}" = -r ]; then
	find "$2" -type f | sort | while IFS= read -r f; do
		head -c 4 "$f" 2> /dev/null | grep -q 'ELF' || continue
		v="$(needs "$f")"
		if over "$v"; then echo "glibc $v: ${f#"$2"/}"; fi
	done
	exit 0
fi

bad=0
for f in "$@"; do
	v="$(needs "$f")"
	if over "$v"; then
		echo "$f needs glibc $v, newer than the A133P SDK baseline $FLOOR" >&2
		bad=1
	fi
done
exit "$bad"
