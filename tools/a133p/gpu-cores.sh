#!/bin/sh
# Host-only A133P GPU builds. Usage: sh tools/a133p/gpu-cores.sh [CORE...]
# A133P_PROFILE=1 builds nodosumi or flycast for profiling on the device: the release recipe and
# optimisation plus debug info, into target-a133p/cores-profile. -g leaves the code alone, so a
# profiling build runs its release build's code.
#
# Nodosumi also gets -fomit-frame-pointer, which Nodosumi's H700 build has from GCC's default and
# zig's clang leaves off on AArch64 (one register more and no frame setup in its C helpers), and
# -fno-math-errno, so sqrtf and the like are the instruction rather than a call on a negative
# input (zig's compiler_rt has a software sqrtf). The rest of its codegen already matches or beats
# the H700's: both -O3, no LTO, no sanitizer traps or outline atomics, and no stack protector
# here where Ubuntu's GCC adds one.
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$root/target-a133p"
cc="$root/tools/a133p/zcc"
cxx="$root/tools/a133p/zcxx"
gpu="${A133P_GPU_LIBS:-$work/gpu}"
export JOBS="${JOBS:-2}"
export CFLAGS="-Wno-error=date-time -I$work/khronos"
profile="${A133P_PROFILE:-}"
[ -z "$profile" ] || CFLAGS="$CFLAGS -g"
export CXXFLAGS="$CFLAGS"
cores="$work/cores"
builddir=build
[ -z "$profile" ] || { cores="$work/cores-profile"; builddir=build-profile; }
[ "$#" -gt 0 ] || set -- nodosumi parallel_n64 flycast vecx
build() (
 name="$1"
 opt=''
 patches=''
 case "$name" in
 nodosumi) repo=cloudwithax/nodosumi; commit=70c5be7b1cd0a2a0d497ad7fcae735784dbbe8a5; flags='-DBUILD_LIBRETRO=ON -DA64_JIT=ON'; renderer='GLES 3.0 (frontend procedure loader)'; opt='-fomit-frame-pointer -fno-math-errno'; patches='' ;;
 parallel_n64) repo=libretro/parallel-n64; commit=a8280501a71c1bcd4c85c5b8bfe75cfa62cd6127; flags='platform=unix-gles ARCH=aarch64 WITH_DYNAREC=aarch64'; renderer='GLES 2.0' ;;
 flycast) repo=flyinghead/flycast; commit=6bb0fc3e18dac4b01694f488c7a71f5930e8a648; flags='-DLIBRETRO=ON -DUSE_GLES=ON -DUSE_VULKAN=OFF -DUSE_OPENMP=OFF -DUSE_HOST_LIBZIP=OFF'; renderer='GLES 3.x (frontend procedure loader)' ;;
 vecx) repo=libretro/libretro-vecx; commit=8f671cc9d737f2890c3ce19e177e2984dcae121f; flags='platform=unix HAS_GPU=0'; renderer=software ;;
 *) echo "no GPU recipe for $name" >&2; exit 2 ;;
 esac
 CFLAGS="$CFLAGS${opt:+ $opt}"
 CXXFLAGS="$CFLAGS"
 if [ "$name" != vecx ]; then
  mkdir -p "$work/khronos"
  for dir in GLES2 GLES3 EGL KHR; do
   cp -R "${KHRONOS_HEADERS:-/usr/include}/$dir" "$work/khronos/"
  done
 fi
 src="$work/src/$name"
 mkdir -p "$src" "$cores" "$work/tmp"
 export TMPDIR="$work/tmp"
 [ -d "$src/.git" ] || git init -q "$src"
 git -C "$src" cat-file -e "$commit^{commit}" 2>/dev/null ||
  git -C "$src" fetch -q --depth 1 "https://github.com/$repo.git" "$commit"
 git -C "$src" checkout -q --detach "$commit"
 cd "$src"
 for p in $patches; do
  patch="$root/tools/a133p/$p"
  if ! git apply --reverse --check "$patch" 2>/dev/null; then
   git apply --check "$patch"
   git apply "$patch"
  fi
 done
 case "$name" in
 nodosumi|flycast)
  if [ "$name" = flycast ]; then
   git submodule update --init --recursive --depth 1
  fi
  cmake -S . -B "$builddir" -DCMAKE_TOOLCHAIN_FILE="$root/tools/a133p/toolchain.cmake" \
   -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS="$CFLAGS" -DCMAKE_CXX_FLAGS="$CXXFLAGS" \
   -DCMAKE_SHARED_LINKER_FLAGS="-L$gpu" $flags
  target=nodosumi
  [ "$name" != flycast ] || target=flycast_libretro
  cmake --build "$builddir" --target "$target" -j"$JOBS"
  output="$builddir/${name}_libretro.so"
  ;;
 parallel_n64)
  [ -z "$profile" ] || { echo "no profiling recipe for $name" >&2; exit 2; }
  [ -f "$gpu/libGLESv2.so" ] || { echo "provide firmware libGLESv2.so in $gpu" >&2; exit 1; }
  make -j"$JOBS" $flags CC="$cc" CXX="$cxx" LD="$cxx" \
   GL_LIB="-L$gpu -lGLESv2" CPUFLAGS='-DARM -DDONT_WANT_ARM_OPTIMIZATIONS -DARM64'
  output=parallel_n64_libretro.so
  ;;
 vecx)
  [ -z "$profile" ] || { echo "no profiling recipe for $name" >&2; exit 2; }
  make -j"$JOBS" $flags CC="$cc" CXX="$cxx" LD="$cxx"
  output=vecx_libretro.so
  ;;
 esac
 readelf -h "$output" | grep -q 'Machine:.*AArch64' ||
  { echo "not an AArch64 ELF: $output" >&2; exit 1; }
 sh "$root/tools/a133p/check-glibc.sh" "$output"
 if readelf -d "$output" | grep -E 'NEEDED.*\[(libGL\.|libOpenGL\.|libGLX\.|libstdc\+\+\.)' ||
    objdump -T "$output" | grep GLIBCXX; then
  echo "unexpected desktop GL or dynamic C++ dependency: $name" >&2
  exit 1
 fi
 # Check direct GPU imports against the real firmware drivers, never host GL.
 # Frontend-loaded procedures (nodosumi/flycast) are not ELF imports.
 symbols="$(mktemp -d "$work/tmp/gpu-symbols.XXXXXX")"
 trap 'rm -rf "$symbols"' EXIT HUP INT TERM
 readelf --dyn-syms --wide "$output" |
  awk '$7 == "UND" && $8 ~ /^(gl|egl)[A-Z]/ {sub(/@.*/, "", $8); print $8}' |
  sort -u >"$symbols/imports"
 if [ -s "$symbols/imports" ]; then
  for lib in libGLESv2.so libEGL.so; do
   [ -f "$gpu/$lib" ] || { echo "provide firmware $lib in $gpu" >&2; exit 1; }
   readelf --dyn-syms --wide "$gpu/$lib"
  done | awk '$7 != "UND" && $8 ~ /^(gl|egl)[A-Z]/ {sub(/@.*/, "", $8); print $8}' |
   sort -u >"$symbols/exports"
  comm -23 "$symbols/imports" "$symbols/exports" >"$symbols/missing"
  if [ -s "$symbols/missing" ]; then
   echo "GPU imports missing from firmware drivers:" >&2
   cat "$symbols/missing" >&2
   exit 1
  fi
 fi
 cp "$output" "$cores/${name}_libretro.so"
 {
  echo "repo=https://github.com/$repo"
  echo "commit=$commit"
  echo "flags=$flags"
  echo "compiler=tools/a133p/zcc,zcxx -target aarch64-linux-gnu.2.23 -mcpu=cortex_a53"
  echo "CFLAGS=$CFLAGS CXXFLAGS=$CXXFLAGS"
  echo "renderer=$renderer"
  case "$name" in
  nodosumi|flycast) echo "CMAKE_BUILD_TYPE=Release CMAKE_SHARED_LINKER_FLAGS=-L$gpu" ;;
  parallel_n64) echo "GL_LIB=-L$gpu -lGLESv2 CPUFLAGS=-DARM -DDONT_WANT_ARM_OPTIMIZATIONS -DARM64" ;;
  esac
  [ -z "$profile" ] || echo "profile=release optimisation plus -g"
  for p in $patches; do
   echo "patch=tools/a133p/$p"
   echo "patch_sha256=$(sha256sum "$root/tools/a133p/$p" | cut -d ' ' -f1)"
  done
 } >"$cores/${name}_libretro.so.meta"
)
if [ "${1:-}" = --build ]; then
 build "$2"
 exit
fi
failed=0
for core do
 if sh "$root/tools/a133p/gpu-cores.sh" --build "$core"; then echo "built $core"; else echo "failed $core" >&2; failed=1; fi
done
exit "$failed"
