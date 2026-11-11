#!/bin/sh
# Install apt packages on GitHub runners, tolerating a broken package mirror.
#
#   tools/ci_apt_install.sh PACKAGE...
#
# The hosted Ubuntu images point apt at azure.archive.ubuntu.com; when that
# name stops resolving (seen on 2026-11-11, failing every release build), we
# retry and then fall back to other official Ubuntu mirrors.
set -u
try() {
    sudo apt-get -o Acquire::Retries=3 update &&
        sudo apt-get -o Acquire::Retries=3 install -y "$@"
}
try "$@" && exit 0
for mirror in http://archive.ubuntu.com/ubuntu http://mirrors.edge.kernel.org/ubuntu; do
    echo "apt install failed; switching package mirror to $mirror"
    for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
        [ -f "$f" ] || continue
        sudo sed -i -E "s#https?://(azure\.archive|archive|security)\.ubuntu\.com/ubuntu/?#$mirror#g; s#https?://mirrors\.edge\.kernel\.org/ubuntu/?#$mirror#g" "$f"
    done
    try "$@" && exit 0
    sleep 10
done
echo "could not install: $*" >&2
exit 1
