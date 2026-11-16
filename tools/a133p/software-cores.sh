#!/bin/sh
# Host-only software cores for the A133P firmware; JOBS is fixed at two.
# Usage: sh tools/a133p/software-cores.sh [CORE...]
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$root/target-a133p"
cc="$root/tools/a133p/zcc"
cxx="$root/tools/a133p/zcxx"
export CFLAGS="-Wno-error=date-time"
export CXXFLAGS="-Wno-error=date-time"
export JOBS=2
export TMPDIR="$work/tmp"
mkdir -p "$TMPDIR" "$work/cores"
[ "$#" -gt 0 ] || set -- gpsp fceumm genesis_plus_gx mednafen_pce_fast race mednafen_wswan handy pokemini gw picodrive gearsystem mednafen_supergrafx stella2014 a5200 prosystem gearcoleco freeintv o2em freechaf potator sameduck fmsx bluemsx vice_x64 fuse cap32 atari800 scummvm
build() (
 name="$1"; sub=; file=Makefile; flags=; output="${1}_libretro.so"
 case "$name" in
	gpsp) repo=libretro/gpsp; commit=7b2dd37394e2822110cacef321e9bb291a305b77; sub=; file=Makefile; flags='CPU_ARCH=arm64 HAVE_DYNAREC=1' ;;
	fceumm) repo=libretro/libretro-fceumm; commit=77468e568d9fe9c48d684ca911f1a3fe7259fdf7; sub=; file=Makefile.libretro; flags='' ;;
	genesis_plus_gx) repo=libretro/Genesis-Plus-GX; commit=b9c2316441978d4015ba30e9a95d9b0e6f8ee981; sub=; file=Makefile.libretro; flags='' ;;
	mednafen_pce_fast) repo=libretro/beetle-pce-fast-libretro; commit=2de1ce1c3f237b161c126c54ba407990bb86d1e9; sub=; file=Makefile; flags='' ;;
	race) repo=libretro/RACE; commit=c660d9ab5aafb47aefff90182ca90d319ad9b077; sub=; file=Makefile; flags='' ;;
	mednafen_wswan) repo=libretro/beetle-wswan-libretro; commit=4b01295838ea89e3f1355bbe4cb5cf98aa6108cd; sub=; file=Makefile; flags='' ;;
	handy) repo=libretro/libretro-handy; commit=bc55d462f0b2d6b073ea93dc552ebd73cec60fd1; sub=; file=Makefile; flags='' ;;
	pokemini) repo=libretro/PokeMini; commit=132111b76343559860532a1ccc094f93f1ed5650; sub=; file=Makefile.libretro; flags='' ;;
	gw) repo=libretro/gw-libretro; commit=91d599b951e7bfe7e040347f58667cba20074adc; sub=; file=Makefile.libretro; flags='' ;;
	picodrive) repo=libretro/picodrive; commit=1890c2932234c9d30f4cd3851d02228baae8f09e; sub=; file=Makefile.libretro; flags='ARCH=aarch64' ;;
	gearsystem) repo=drhelius/Gearsystem; commit=1bd547250831111b512f80b59d00c2f1b90ba03e; sub=platforms/libretro; file=Makefile; flags='' ;;
	mednafen_supergrafx) repo=libretro/beetle-supergrafx-libretro; commit=9c11dcd213a79d57d2c86313971631af801f2cdc; sub=; file=Makefile; flags='' ;;
	stella2014) repo=libretro/stella2014-libretro; commit=7d1361e407e63f29e52892655069e5fb4096e691; sub=; file=Makefile; flags='' ;;
	a5200) repo=libretro/a5200; commit=40c6f2f1ad4a3145b328d5baaf010fae6c7e752b; sub=; file=Makefile; flags='' ;;
	prosystem) repo=libretro/prosystem-libretro; commit=8a88014287c7a01cd568067e5a557d0a2b2a051f; sub=; file=Makefile; flags='' ;;
	gearcoleco) repo=drhelius/Gearcoleco; commit=19390c77c1eb4deb918b25da2768f0d1e9cd6e55; sub=platforms/libretro; file=Makefile; flags='' ;;
	freeintv) repo=libretro/FreeIntv; commit=ef3e0fe322bec62a7f916c0bb0834c08c348d0b4; sub=; file=Makefile; flags='' ;;
	o2em) repo=libretro/libretro-o2em; commit=679d6fec04963f6e70a7ec217e3d0ebb1fe472fc; sub=; file=Makefile; flags='' ;;
	freechaf) repo=libretro/FreeChaF; commit=76c7a84f1f7e80f3e6f2bba96fe100cb24e99124; sub=; file=Makefile; flags='' ;;
	potator) repo=libretro/potator; commit=227c5f6f3ce74d32e9002ce24c1420288559a860; sub=platform/libretro; file=Makefile; flags='' ;;
	sameduck) repo=libretro/sameduck; commit=040e19c379f00dae10afe0eb0c1b2b019b985bbe; sub=libretro; file=Makefile; flags='' ;;
	fmsx) repo=libretro/fmsx-libretro; commit=4de11755ce4f196ac1c8a7bb20bb4eccbc87a7d4; sub=; file=Makefile; flags='' ;;
	bluemsx) repo=libretro/blueMSX-libretro; commit=e3086eb5d36d77fa11704cf53dc176686e70127d; sub=; file=Makefile.libretro; flags='' ;;
	vice_x64) repo=libretro/vice-libretro; commit=76cde0b2cc4e52106618f2cc5d9d1112d9205c89; sub=; file=Makefile; flags='' ;;
	fuse) repo=libretro/fuse-libretro; commit=e997e2bc32c888348f862f69f2c53babfedf7791; sub=; file=Makefile.libretro; flags='' ;;
	cap32) repo=libretro/libretro-cap32; commit=06ad0da5d003ab8f2359300ebce4cf31d4c55b6e; sub=; file=Makefile; flags='' ;;
	atari800) repo=libretro/libretro-atari800; commit=4e7fbc73765c1a9670c7506616046ad1d4ccda51; sub=; file=Makefile; flags='' ;;
	scummvm) repo=libretro/scummvm; commit=e0ddc60bfabb1efdf1e135006c8914ca59d0596c; sub=backends/platform/libretro; file=Makefile; flags='BUILD_64BIT=1 FORCE_OPENGLES2=1' ;;
 *) echo "no software recipe for $name" >&2; exit 2 ;;
 esac
 src="$work/src/$name"
 mkdir -p "$src"
 [ -d "$src/.git" ] || git init -q "$src"
 git -C "$src" cat-file -e "$commit^{commit}" 2>/dev/null ||
  git -C "$src" fetch -q --depth 1 "https://github.com/$repo.git" "$commit"
 git -C "$src" checkout -q --detach "$commit"
 if [ -f "$src/.gitmodules" ]; then
  git -C "$src" submodule update --init --recursive --depth 1
 fi
 if [ "$name" = bluemsx ]; then
  patch="$root/tools/a133p/bluemsx-zlib.patch"
  git -C "$src" apply --unidiff-zero --reverse --check "$patch" 2>/dev/null ||
   git -C "$src" apply --unidiff-zero "$patch"
  # Legacy callbacks use typed object pointers where their API uses void *;
  # these have the same AArch64 calling convention.
  cc="$cc -Wno-error=incompatible-function-pointer-types"
 fi
 cd "$src${sub:+/$sub}"
 # Word splitting expands the pinned recipe's make assignments.
 case "$name" in
 fceumm) link=src/drivers/libretro/link.T ;;
 race|handy) link=libretro/link.T ;;
 gw|fuse) link=build/link.T ;;
 fmsx|bluemsx|cap32|atari800) link=link.T ;;
 *) link= ;;
 esac
 if [ -n "$link" ]; then
  shared="-shared -Wl,--no-undefined -Wl,--version-script=$link"
  make -f "$file" -j2 platform=unix CC="$cc" CXX="$cxx" LD="$cxx" SHARED="$shared" $flags
 else
  make -f "$file" -j2 platform=unix CC="$cc" CXX="$cxx" LD="$cxx" $flags
 fi
 sh "$root/tools/a133p/check-glibc.sh" "$output"
 readelf -h "$output" | grep -q 'Machine:.*AArch64'
 if objdump -T "$output" | grep -q GLIBCXX_; then
  echo "$name imports GLIBCXX" >&2; exit 1
 fi
 cp "$output" "$work/cores/${name}_libretro.so"
 {
  echo "repo=https://github.com/$repo"
  echo "commit=$commit"
  echo "flags=platform=unix $flags"
  echo "compiler=tools/a133p/zcc,zcxx -target aarch64-linux-gnu.2.23 -mcpu=cortex_a53"
  echo "CFLAGS=$CFLAGS CXXFLAGS=$CXXFLAGS"
  [ "$name" != bluemsx ] || echo "CC=$cc"
  echo "jobs=2"
  [ -z "$link" ] || echo "SHARED=$shared"
  [ "$name" != bluemsx ] || echo "patch=tools/a133p/bluemsx-zlib.patch sha256:$(sha256sum "$patch" | cut -d' ' -f1)"
 } >"$work/cores/${name}_libretro.so.meta"
)
if [ "${1:-}" = --build ]; then build "$2"; exit; fi
failed=0
for core do
 if sh "$root/tools/a133p/software-cores.sh" --build "$core"; then
  echo "built $core"
 else
  echo "failed $core" >&2; failed=1
 fi
done
exit "$failed"
