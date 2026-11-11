#!/usr/bin/env bash
# Install mingw-w64 (Ubuntu noble packages) for the Windows cross build,
# tolerating outages of individual Ubuntu mirrors.
#
#   tools/ci_install_mingw.sh [mirror...]
#
# Every mirror below serves the identical Ubuntu packages, so the toolchain,
# and with it the release binaries' SHA-256, does not depend on which mirror
# answered. apt is pointed at one mirror at a time through a private
# sources list, so unrelated runner repositories cannot fail the step.
set -uo pipefail

mirrors=("$@")
if [ ${#mirrors[@]} -eq 0 ]; then
    mirrors=(
        http://mirrors.edge.kernel.org/ubuntu
        http://mirrors.mit.edu/ubuntu
        http://archive.ubuntu.com/ubuntu
        http://mirror.math.princeton.edu/pub/ubuntu
        http://ftp.halifax.rwth-aachen.de/ubuntu
        http://azure.archive.ubuntu.com/ubuntu
    )
fi
codename=$(. /etc/os-release && echo "${VERSION_CODENAME:-noble}")
list=$(mktemp)
opts=(-o Acquire::Retries=2 -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15
      -o "Dir::Etc::sourcelist=$list" -o Dir::Etc::sourceparts=-
      -o APT::Get::List-Cleanup=0)

if command -v x86_64-w64-mingw32-gcc >/dev/null; then
    echo "mingw-w64 already installed"
    exit 0
fi
for m in "${mirrors[@]}"; do
    echo "::group::mingw-w64 from $m"
    printf 'deb %s %s main universe\ndeb %s %s-updates main universe\ndeb %s %s-security main universe\n' \
        "$m" "$codename" "$m" "$codename" "$m" "$codename" > "$list"
    if timeout 180 sudo apt-get "${opts[@]}" update \
        && timeout 300 sudo apt-get "${opts[@]}" install -y mingw-w64; then
        echo "::endgroup::"
        x86_64-w64-mingw32-gcc --version | head -1
        exit 0
    fi
    echo "::endgroup::"
    echo "mirror $m failed, trying the next one"
done
echo "mingw-w64 could not be installed from any mirror" >&2
exit 1
