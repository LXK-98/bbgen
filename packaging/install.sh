#!/bin/sh
# Install bbgen and bbgen studio: the two programs, bbgen studio's menu entry and icon, the docs and examples,
# and the Claude Code skill. In an unpacked release package it installs that package; run on its own, as in
#
#     curl -fsSL https://github.com/LXK-98/bbgen/releases/latest/download/install.sh | sh
#
# it downloads the latest release's package first. Run it again to update; --uninstall takes it all out again.
set -eu

repo=LXK-98/bbgen
usage="usage: install.sh [--prefix DIR] [--no-skill] [--uninstall]

  --prefix DIR   install into DIR/bin and DIR/share (default: ~/.local, no root needed)
  --no-skill     leave out the Claude Code skill (~/.claude/skills/breadboard)
  --uninstall    remove what an earlier install.sh put in DIR (and the skill)

Run on its own, it installs the latest release from github.com/$repo, or the release whose
download folder \$BBGEN_RELEASE_URL names."

# the arguments are read without being used up: a download hands them on to the package's install.sh
prefix="$HOME/.local"
skill=yes
uninstall=no
want=
for arg in "$@"; do
    if [ "$want" = prefix ]; then
        prefix="$arg"
        want=
        continue
    fi
    case "$arg" in
        --prefix) want=prefix ;;
        --prefix=*) prefix="${arg#--prefix=}" ;;
        --no-skill) skill=no ;;
        --uninstall) uninstall=yes ;;
        -h|--help) echo "$usage"; exit 0 ;;
        *) echo "$usage" >&2; exit 2 ;;
    esac
done
if [ -n "$want" ]; then
    echo "$usage" >&2
    exit 2
fi
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)
manifest="$prefix/share/bbgen/installed-files"
skills="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/skills"

# Remove every file the manifest lists, and the folders of ours that leaves empty. Not a file that's become a link,
# or one in a folder that has: that's someone else's now (the skill linked in from a checkout, say).
remove_installed() {
    [ -f "$manifest" ] || return 0
    while IFS= read -r f; do
        if [ ! -L "$f" ] && [ ! -L "$(dirname "$f")" ]; then
            rm -f "$f"
        fi
    done < "$manifest"
    rm -f "$manifest"
    # deepest first: examples/ before share/bbgen/
    find "$prefix/share/bbgen" -depth -type d -empty -delete 2>/dev/null || true
    rmdir "$skills/breadboard" 2>/dev/null || true
}

# The desktop's caches of menu entries and icons, so bbgen studio shows up in (or goes from) the menu at once.
refresh_menus() {
    if command -v update-desktop-database > /dev/null && [ -d "$prefix/share/applications" ]; then
        update-desktop-database -q "$prefix/share/applications" 2> /dev/null || true
    fi
    if command -v gtk-update-icon-cache > /dev/null && [ -d "$prefix/share/icons/hicolor" ]; then
        gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2> /dev/null || true
    fi
}

if [ "$uninstall" = yes ]; then
    if [ ! -f "$manifest" ]; then
        echo "nothing to uninstall: install.sh hasn't installed bbgen in $prefix"
        exit 1
    fi
    remove_installed
    refresh_menus
    echo "uninstalled bbgen from $prefix"
    exit 0
fi

# Not in an unpacked package (piped into sh, say): download the release's package and run its install.sh.
here=$(cd "$(dirname "$0")" && pwd)
case "$0" in
    */install.sh | install.sh) packaged=$([ -x "$here/bin/bbgen" ] && echo yes || echo no) ;;
    *) packaged=no ;;
esac
if [ "$packaged" = no ]; then
    if [ "$(uname -s)" != Linux ] || [ "$(uname -m)" != x86_64 ]; then
        echo "bbgen has a package for Linux on x86_64 only so far (this is $(uname -s) on $(uname -m)); other" >&2
        echo "systems may follow. Until then, build it from source:" >&2
        echo "  https://github.com/$repo/blob/main/docs/development.md" >&2
        exit 1
    fi
    url="${BBGEN_RELEASE_URL:-https://github.com/$repo/releases/latest/download}"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    fetch() {
        if command -v curl > /dev/null; then
            curl -fsSL "$1" -o "$2"
        elif command -v wget > /dev/null; then
            wget -q "$1" -O "$2"
        else
            echo "install.sh needs curl or wget to download bbgen" >&2
            exit 1
        fi
    }
    echo "downloading bbgen from $url"
    fetch "$url/bbgen-x86_64-linux.tar.gz" "$tmp/bbgen-x86_64-linux.tar.gz"
    fetch "$url/SHA256SUMS" "$tmp/SHA256SUMS"
    # a download that can't be checked isn't installed
    if command -v sha256sum > /dev/null; then
        sha256="sha256sum"
    elif command -v shasum > /dev/null; then
        sha256="shasum -a 256"
    else
        echo "install.sh needs sha256sum or shasum to check the download: not installed" >&2
        exit 1
    fi
    (cd "$tmp" && grep ' bbgen-x86_64-linux.tar.gz$' SHA256SUMS | $sha256 -c - > /dev/null) || {
        echo "the download doesn't match its checksum: not installed" >&2
        exit 1
    }
    tar -xzf "$tmp/bbgen-x86_64-linux.tar.gz" -C "$tmp"
    sh "$tmp"/bbgen-*-x86_64-linux/install.sh "$@"
    exit
fi

# an update: what the last install put there goes first, so nothing it had and this one doesn't stays behind
remove_installed

# Copy one file to dest, replacing (not writing through) a link that's there: ~/.local/bin/bbgen may link to a
# checkout's launcher.
put() {
    src="$1" dest="$2" mode="$3"
    mkdir -p "$(dirname "$dest")"
    if [ -L "$dest" ]; then
        echo "replacing the link $dest (to $(readlink "$dest"))"
    fi
    rm -f "$dest"
    cp "$src" "$dest"
    chmod "$mode" "$dest"
    echo "$dest" >> "$manifest.new"
}

mkdir -p "$prefix/share/bbgen"
: > "$manifest.new"
cd "$here"
for f in bin/*; do
    put "$f" "$prefix/$f" 755
done
find share -type f | sort | while IFS= read -r f; do
    put "$f" "$prefix/$f" 644
done
# the menu entry runs bbgen studio by its full path: a desktop session's PATH may not have $prefix/bin
desktop="$prefix/share/applications/bbgen-studio.desktop"
sed "s|^Exec=bbgen-studio |Exec=\"$prefix/bin/bbgen-studio\" |" share/applications/bbgen-studio.desktop > "$desktop"

if [ "$skill" = yes ]; then
    dest="$skills/breadboard"
    if [ "$(id -u)" = 0 ]; then
        echo "skill: not installed as root (it's per user); as yourself, run install.sh --prefix $prefix again"
    elif [ -L "$dest" ] && [ -e "$dest" ]; then
        # someone linked the skill in by hand, usually from a checkout: theirs to keep
        echo "skill: kept $dest, which links to $(readlink "$dest")"
    else
        [ -L "$dest" ] && rm -f "$dest"
        put share/bbgen/skill/SKILL.md "$dest/SKILL.md" 644
    fi
fi
mv "$manifest.new" "$manifest"
refresh_menus

echo "installed bbgen $("$prefix/bin/bbgen" --version 2>/dev/null | cut -d' ' -f2) in $prefix"

# what's missing for it to run, or to run as installed
if ! "$prefix/bin/bbgen" --version > /dev/null 2>&1; then
    echo
    echo "bbgen doesn't run here:"
    "$prefix/bin/bbgen" --version 2>&1 | sed 's/^/  /' || true
fi
missing=$(ldd "$prefix/bin/bbgen-studio" 2>/dev/null | sed -n 's/^[[:space:]]*\([^ ]*\) => not found.*/\1/p' | tr '\n' ' ')
if [ -n "$missing" ]; then
    echo
    echo "bbgen studio needs WebKitGTK 4.1, which isn't installed (missing: $missing). Install it with"
    echo "  Fedora:        sudo dnf install webkit2gtk4.1"
    echo "  Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0"
    echo "  Arch:          sudo pacman -S webkit2gtk-4.1"
fi
case ":$PATH:" in
    *":$prefix/bin:"*)
        found=$(command -v bbgen || true)
        if [ "$found" != "$prefix/bin/bbgen" ]; then
            echo
            echo "note: 'bbgen' runs $found, which comes before $prefix/bin on your PATH"
        fi
        ;;
    *)
        echo
        echo "$prefix/bin isn't on your PATH: add it (e.g. in ~/.profile) to run bbgen by name:"
        echo "  export PATH=\"$prefix/bin:\$PATH\""
        ;;
esac
if ! command -v magick > /dev/null && ! command -v rsvg-convert > /dev/null; then
    echo
    echo "optional: bbgen writes PNGs with ImageMagick or rsvg-convert, and videos with ImageMagick and ffmpeg"
fi
