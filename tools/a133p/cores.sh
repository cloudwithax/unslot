#!/bin/sh
# Rebuild shipped cores for Cortex-A53 against the SDK's glibc 2.23.
# Usage: sh tools/a133p/cores.sh [CORE...]; JOBS defaults to 2.
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$root/target-a133p"
cc="$root/tools/a133p/zcc"
cxx="$root/tools/a133p/zcxx"
export JOBS="${JOBS:-2}"
export CFLAGS="-Wno-error=date-time"
export CXXFLAGS="-Wno-error=date-time"
[ "$#" -gt 0 ] || set -- nodosumi parallel_n64 flycast vecx \
	pcsx_rearmed fbneo snes9x neocd mame2003_plus dosbox_pure puae2021 hatarib \
	mednafen_pcfx np2kai px68k retro8 tic80 mednafen_vb \
	gpsp fceumm genesis_plus_gx mednafen_pce_fast race mednafen_wswan handy \
	pokemini gw picodrive gearsystem mednafen_supergrafx stella2014 a5200 \
	prosystem gearcoleco freeintv o2em freechaf potator sameduck fmsx bluemsx \
	vice_x64 fuse cap32 atari800 scummvm
build() (
	name="$1" sub= file=Makefile flags= output="${1}_libretro.so"
	case "$name" in
	nodosumi|parallel_n64|flycast|vecx)
		exec sh "$root/tools/a133p/gpu-cores.sh" "$name" ;;
	gpsp|fceumm|genesis_plus_gx|mednafen_pce_fast|race|mednafen_wswan|handy|pokemini|gw|picodrive|gearsystem|mednafen_supergrafx|stella2014|a5200|prosystem|gearcoleco|freeintv|o2em|freechaf|potator|sameduck|fmsx|bluemsx|vice_x64|fuse|cap32|atari800|scummvm)
		exec sh "$root/tools/a133p/software-cores.sh" "$name" ;;
	pcsx_rearmed) repo=libretro/pcsx_rearmed; commit=7b0e8f8fadfbce3d170d195b9d9eecaabfa08c8e; file=Makefile.libretro; flags='ARCH=aarch64 DYNAREC=ari64 USE_MINIZ=1 HAVE_PHYSICAL_CDROM=0' ;;
	fbneo) repo=libretro/FBNeo; commit=65e63fac2fc6903cc337906db13ea17feeb168dd; sub=src/burner/libretro ;;
	snes9x) repo=libretro/snes9x; commit=fae2fea08f74180759ef540ee94259213f503480; sub=libretro; flags='arch=arm64 LTO=' ;;
	neocd) repo=libretro/neocd_libretro; commit=b1e04c738cb48a1dae0574b8877f6a116d270ca1 ;;
	mame2003_plus) repo=libretro/mame2003-plus-libretro; commit=fde3d46fc48fcc0704797e96ac4613bb47ff2c57 ;;
	dosbox_pure) repo=libretro/dosbox-pure; commit=e39497e7c2a32c67c26e6598dd14f6d58c384bfd; flags='CPUFLAGS=-DPAGESIZE=4096 STRIPCMD=true' ;;
	puae2021) repo=libretro/libretro-uae; commit=5dbac2bf90b2dddf9d3f8d5b2ccad0651f798c1d; flags='TARGET_NAME=puae2021' ;;
	mednafen_pcfx) repo=libretro/beetle-pcfx-libretro; commit=c1650bad5fbdcc3c3ccd45e29e3983e1ccf16cff ;;
	np2kai) repo=libretro/NP2kai; commit=1ea561cd7fd62b33f4e7b3f1c2e30fc3e6089f28; sub=sdl; file=Makefile.libretro ;;
	hatarib) repo=bbbradsmith/hatariB; commit=5f05733fc21903dce7d978bd0d7aae445ffe1185; file=makefile.libretro; flags="WERROR= MULTITHREAD=-j${JOBS:-4} CMAKE_TOOLCHAIN_FILE=tools/a133p/toolchain.cmake" ;;
	tic80) repo=libretro/TIC-80; commit=14fd99edc29962778bc88d81773288ffc6e8aa53; output=build/bin/tic80_libretro.so
		flags='-DBUILD_LIBRETRO=ON -D__LIBRETRO__=ON -DBUILD_PLAYER=OFF -DBUILD_PRO=OFF -DBUILD_SDL=OFF -DBUILD_TOOLS=OFF -DBUILD_TOUCH_INPUT=OFF -DBUILD_STATIC=ON -DBUILD_WITH_ALL=OFF -DBUILD_WITH_LUA=ON -DBUILD_WITH_MOON=ON -DBUILD_WITH_FENNEL=ON -DBUILD_WITH_WREN=ON -DBUILD_WITH_JS=ON -DBUILD_EDITORS=OFF -DCMAKE_BUILD_TYPE=Release' ;;
	px68k) repo=libretro/px68k-libretro; commit=3f4f542a39a89ca63c626326fa50c4f0ea5871d7; file=Makefile.libretro ;;
	retro8) repo=libretro/retro8; commit=ddc06a142398ee9755894b3f0bb17c8dc428151d ;;
	mednafen_vb) repo=libretro/beetle-vb-libretro; commit=d5ae54c8e23b573a3b34e888b2d48a588027e92c; flags='HAVE_OPENGL=0 HAVE_GLES=0' ;;
	*) echo "no recipe for $name" >&2; exit 2 ;;
	esac
	src="$work/src/$name"
	mkdir -p "$src" "$work/cores"
	[ -d "$src/.git" ] || git init -q "$src"
	git -C "$src" cat-file -e "$commit^{commit}" 2>/dev/null ||
		git -C "$src" fetch -q --depth 1 "https://github.com/$repo.git" "$commit"
	git -C "$src" checkout -q --detach "$commit"
	cd "$src${sub:+/$sub}"
	# Word splitting is intentional: each recipe contains make variable assignments.
	if [ "$name" = tic80 ]; then
		git -C "$src" submodule update --init --recursive --depth 1
		cmake -S core -B build -DCMAKE_TOOLCHAIN_FILE="$root/tools/a133p/toolchain.cmake" \
			-DCMAKE_C_FLAGS_RELEASE="-O3 -DNDEBUG -fno-lto" \
			-DCMAKE_CXX_FLAGS_RELEASE="-O3 -DNDEBUG -fno-lto" $flags
		cmake --build build --target tic80_libretro -j"${JOBS:-4}"
	elif [ "$name" = hatarib ]; then
		git -C "$src" submodule update --init --depth 1
		make -f "$file" -j"${JOBS:-4}" platform=unix CC="$cc" CXX="$cxx" \
			WERROR= MULTITHREAD="-j${JOBS:-4}" \
			CMAKEFLAGS_EXTRA="-DCMAKE_TOOLCHAIN_FILE=$root/tools/a133p/toolchain.cmake"
	elif [ "$name" = fbneo ]; then
		make -f "$file" -j"${JOBS:-4}" platform=unix CC="$cc" CXX="$cxx" LD="$cxx" \
			SHARED="-shared -Wl,--no-undefined -Wl,--version-script=../../burner/libretro/link.T"
	else
		make -f "$file" -j"${JOBS:-4}" platform=unix CC="$cc" CXX="$cxx" LD="$cxx" $flags
	fi
	sh "$root/tools/a133p/check-glibc.sh" "$output"
	cp "$output" "$work/cores/${name}_libretro.so"
	{
		echo "repo=https://github.com/$repo"
		echo "commit=$commit"
		echo "flags=platform=unix $flags"
		echo "compiler=tools/a133p/zcc,zcxx -target aarch64-linux-gnu.2.23 -mcpu=cortex_a53"
		echo "CFLAGS=$CFLAGS CXXFLAGS=$CXXFLAGS"
		[ "$name" != fbneo ] || echo "SHARED=-shared -Wl,--no-undefined -Wl,--version-script=../../burner/libretro/link.T"
		[ "$name" != tic80 ] || echo "CMAKE_C_FLAGS_RELEASE=CMAKE_CXX_FLAGS_RELEASE=-O3 -DNDEBUG -fno-lto"
	} >"$work/cores/${name}_libretro.so.meta"
)
if [ "${1:-}" = --build ]; then
	build "$2"
	exit
fi
failed=0
for core do
	if sh "$root/tools/a133p/cores.sh" --build "$core"; then echo "built $core"; else echo "failed $core" >&2; failed=1; fi
done
exit "$failed"
