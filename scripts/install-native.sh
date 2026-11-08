#!/bin/sh
# Staged native installation only. Never builds or invokes the installed binary.
set -eu

fail() { printf '%s\n' "install-native: $*" >&2; exit 1; }
usage() {
    printf '%s\n' \
        'Usage: sh scripts/install-native.sh --binary /absolute/prebuilt/coven --prefix /absolute/new/prefix [--examples /absolute/examples]' \
        'Installs bin/coven and share/coven/examples, without Cargo, Python, wrappers, or runners.' \
        'The prefix must not exist, even as an empty directory. Its parent must already exist.' \
        'Prefix and examples root symlinks and .. components are refused. Ancestors must be trusted and stable.' \
        'Failures after reservation preserve a partial prefix; retries refuse it. No cleanup or rollback is performed.' \
        'Defaults to the examples beside this script in the source distribution. No docs are installed.'
}

# Strip directory suffixes before checking the leaf, without resolving symlinks.
normalize() {
    case "$1" in /*) ;; *) fail 'paths must be absolute' ;; esac
    case "$1/" in */../*) fail '.. path components are not allowed' ;; esac
    path=$1
    while [ "$path" != / ]; do
        case "$path" in
            */) path=${path%/} ;;
            */.) path=${path%/.} ;;
            *) break ;;
        esac
        [ -n "$path" ] || path=/
    done
    printf '%s\n' "$path"
}

binary= prefix= examples=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --binary|--prefix|--examples)
            [ "$#" -ge 2 ] || fail "missing value for $1"
            case "$1" in
                --binary) [ -z "$binary" ] || fail 'duplicate --binary'; binary=$2 ;;
                --prefix) [ -z "$prefix" ] || fail 'duplicate --prefix'; prefix=$2 ;;
                --examples) [ -z "$examples" ] || fail 'duplicate --examples'; examples=$2 ;;
            esac
            shift 2 ;;
        --help) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done
[ -n "$binary" ] && [ -n "$prefix" ] || { usage >&2; exit 1; }
binary=$(normalize "$binary")
prefix=$(normalize "$prefix")
[ ! -L "$binary" ] && [ -f "$binary" ] && [ -x "$binary" ] || fail '--binary must be a regular executable, not a symlink'
if [ -z "$examples" ]; then
    case "$0" in */*) script_dir=${0%/*} ;; *) script_dir=. ;; esac
    distribution=$(CDPATH= cd -- "$script_dir/.." && pwd -P)
    examples=$distribution/examples
fi
examples=$(normalize "$examples")
[ ! -L "$examples" ] && [ -d "$examples" ] || fail '--examples must be a directory, not a symlink'

[ ! -L "$prefix" ] || fail 'prefix must not be a symlink'
[ ! -e "$prefix" ] || fail 'prefix must not exist; no upgrade or cleanup is performed'
parent=${prefix%/*}
[ -n "$parent" ] || parent=/
[ ! -L "$parent" ] && [ -d "$parent" ] || fail 'prefix parent must be an existing directory, not a symlink'

# These are installer host tools only; Coven has no shell/coreutils dependency.
command -v install >/dev/null || fail 'install tool not found'
command -v mkdir >/dev/null || fail 'mkdir tool not found'
command -v cp >/dev/null || fail 'cp tool not found'
# Only the installer that creates this directory may write inside it. In
# particular, never use mkdir -p here or adopt a racing installer's prefix.
mkdir "$prefix" || fail 'could not exclusively reserve prefix; nothing installed by this attempt'
partial() { fail "partial installation preserved at $prefix; no rollback performed; retry requires a new prefix"; }
mkdir -p "$prefix/bin" "$prefix/share/coven/examples" || partial
install -m 755 "$binary" "$prefix/bin/coven" || partial
cp -R "$examples/." "$prefix/share/coven/examples/" || partial
printf '%s\n' "Installed native coven in $prefix (no existing installation changed)."
