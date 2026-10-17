# The box cores/gles/build.sh runs in: amd64 Debian bullseye cross-compiling for aarch64, with
# the aarch64 GLES, EGL, zlib and libpng headers the GPU cores build against. Bullseye for its
# glibc, older than the device's; its packages now live on Debian's archive. CMake from pip,
# as Flycast wants a newer one than bullseye's.
FROM debian:bullseye
RUN printf '%s\n' \
      'deb http://archive.debian.org/debian bullseye main' \
      'deb http://archive.debian.org/debian bullseye-updates main' \
      'deb http://archive.debian.org/debian-security bullseye-security main' \
      > /etc/apt/sources.list \
    && rm -f /etc/apt/sources.list.d/* \
    && echo 'Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/99archive \
    && dpkg --add-architecture arm64 && apt-get update && apt-get install -y --no-install-recommends \
    crossbuild-essential-arm64 git ca-certificates make python3 python3-pip pkg-config \
    libgles-dev:arm64 libegl-dev:arm64 zlib1g-dev:arm64 libpng-dev:arm64 \
    && rm -rf /var/lib/apt/lists/* \
    && pip3 install --no-cache-dir 'cmake>=3.27,<3.31'
