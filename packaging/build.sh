#!/bin/sh
# Build bbgen and bbgen studio and make a release's files in target/dist:
#
#   bbgen-x86_64-linux.tar.gz   the package: bin/, share/ and install.sh, in bbgen-VERSION-x86_64-linux/
#   install.sh                  the installer on its own, which downloads the package (`curl ... | sh`)
#   SHA256SUMS                  the package's checksum, which that installer checks
#
# Needs what building each needs: cargo, and for the studio pnpm and Tauri's system libraries (docs/development.md);
# and python3 for the third-party notices. The binaries need the glibc they were built against or newer, so build
# on the oldest system the package should run on: the release workflow builds on Ubuntu 22.04 (glibc 2.35).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
arch=$(uname -m)
name="bbgen-$version-$arch-linux"
out="$root/target/dist"
pkg="$out/$name"

echo "building bbgen $version" >&2
cargo build --release --locked --quiet --manifest-path "$root/Cargo.toml"
echo "building bbgen studio" >&2
(cd "$root/studio" && pnpm install --frozen-lockfile --silent)
rm -rf "$pkg" "$out/bbgen-$arch-linux.tar.gz"
mkdir -p "$pkg/bin" "$pkg/share/applications" "$pkg/share/bbgen/skill"
# before the studio's own build, which leaves no source maps: the notices read them
python3 "$root/packaging/notices.py" "$pkg/share/bbgen/THIRD-PARTY-NOTICES.md"
(cd "$root/studio" && pnpm tauri build --no-bundle -- --locked)

# release builds keep debug info for profiling (Cargo.toml); the package doesn't need it
strip --strip-debug -o "$pkg/bin/bbgen" "$root/target/release/bbgen"
strip --strip-debug -o "$pkg/bin/bbgen-studio" "$root/studio/src-tauri/target/release/bbgen-studio"
cp "$root/packaging/bbgen-studio.desktop" "$pkg/share/applications/"
icons="$root/studio/src-tauri/icons"
for size in 32x32:32x32.png 64x64:64x64.png 128x128:128x128.png 256x256:128x128@2x.png 512x512:icon.png; do
    mkdir -p "$pkg/share/icons/hicolor/${size%%:*}/apps"
    cp "$icons/${size#*:}" "$pkg/share/icons/hicolor/${size%%:*}/apps/bbgen-studio.png"
done
mkdir -p "$pkg/share/icons/hicolor/scalable/apps"
cp "$icons/icon.svg" "$pkg/share/icons/hicolor/scalable/apps/bbgen-studio.svg"
# the README keeps its links to docs/ and examples/ working beside it
cp "$root/README.md" "$root/CHANGELOG.md" "$root/LICENSE" "$root/THIRD-PARTY.md" "$pkg/share/bbgen/"
mkdir -p "$pkg/share/bbgen/docs"
cp "$root"/docs/*.md "$pkg/share/bbgen/docs/"
cp -r "$root/examples" "$pkg/share/bbgen/"
cp "$root/skill/SKILL.md" "$pkg/share/bbgen/skill/"
cp "$root/packaging/install.sh" "$pkg/"
chmod 755 "$pkg/install.sh"

tar -C "$out" -czf "$out/bbgen-$arch-linux.tar.gz" "$name"
cp "$root/packaging/install.sh" "$out/install.sh"
(cd "$out" && sha256sum "bbgen-$arch-linux.tar.gz" > SHA256SUMS)
glibc=$(objdump -T "$pkg/bin/bbgen" "$pkg/bin/bbgen-studio" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -n 1)
echo "wrote $out/bbgen-$arch-linux.tar.gz ($name, needs ${glibc:-an unknown glibc} or newer), install.sh and SHA256SUMS" >&2
