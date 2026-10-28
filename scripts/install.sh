#!/bin/sh
# Explicitly trusted native helper adapter, never an archive extractor.
set -eu

fail() { printf '%s\n' "kk installer: $*" >&2; exit 1; }
usage() {
    printf '%s\n' \
        'Usage: sh install.sh --version VERSION --commit COMMIT [--prefix ABSOLUTE_PATH]' \
        '                     [--archive ABSOLUTE_PATH [--bundle ABSOLUTE_PATH]]' \
        '                     --helper ABSOLUTE_ALREADY_TRUSTED_KK [--rollback]' \
        'Requires separately installed gh (attestation), ldconfig, and curl for downloads.' \
        'Without --helper, installation is BLOCKED before any effects.' \
        'The helper owns download, gh verification, bounded validation and activation.' \
        'Local archives must pass the same provenance/content gates. Profiles are untouched.'
}

version='' commit='' archive='' bundle='' helper='' rollback='' prefix=${HOME:?HOME must be set}/.local
seen=' '
while [ "$#" -gt 0 ]; do
    key=$1
    case "$key" in
        --help|-h) usage; exit 0 ;;
        --rollback)
            [ -z "$rollback" ] || fail 'duplicate --rollback'
            rollback=yes
            shift ;;
        --version|--commit|--prefix|--archive|--bundle|--helper)
            case "$seen" in *" $key "*) fail "duplicate argument: $key" ;; esac
            seen="$seen$key "
            [ "$#" -ge 2 ] || fail "missing value for $key"
            [ -n "$2" ] || fail "empty value for $key"
            case "$2" in --*) fail "missing value for $key" ;; esac
            case "$key" in
                --version) version=$2 ;;
                --commit) commit=$2 ;;
                --prefix) prefix=$2 ;;
                --archive) archive=$2 ;;
                --bundle) bundle=$2 ;;
                --helper) helper=$2 ;;
            esac
            shift 2 ;;
        *) fail "unknown argument: $key" ;;
    esac
done

# Literal release selectors only: never evaluate user input or query a moving latest tag.
case "$version" in ''|*[!0-9A-Za-z.-]*|.*|*.) fail 'invalid version (no v prefix)' ;; esac
[ "${#version}" -le 64 ] || fail 'version is too long'
base=${version%%-*}
case "$base" in *[!0-9.]*) fail 'version base must be three decimal components' ;; esac
old_ifs=$IFS
IFS=.
set -f
# Deliberate decimal-component splitting; alphabet checked above, globbing disabled.
# shellcheck disable=SC2086
set -- $base
IFS=$old_ifs
[ "$#" -eq 3 ] || fail 'version must be three decimal components'
for component do
    case "$component" in ''|0[0-9]*) fail 'version components must be canonical decimal numbers' ;; esac
done
if [ -z "$rollback" ]; then
    case "$commit" in ''|*[!0-9a-f]*) fail 'commit must be a full lowercase 40-character SHA' ;; esac
    [ "${#commit}" -eq 40 ] || fail 'commit must be a full lowercase 40-character SHA'
else
    [ -z "$commit$archive" ] || fail '--rollback does not accept commit or archive'
fi
for path in "$prefix" "${archive:-/}" "${bundle:-/}" "${helper:-/}"; do
    case "$path" in /*) ;; *) fail 'paths must be absolute' ;; esac
done
[ -z "$bundle" ] || [ -n "$archive$rollback" ] || fail '--bundle requires --archive or --rollback'

system=$(uname -s) || fail 'cannot detect operating system'
machine=$(uname -m) || fail 'cannot detect architecture'
case "$system" in
    Linux) ;;
    MINGW*|MSYS*|CYGWIN*|Windows*) fail 'native Windows release is not shipped; refusing installation' ;;
    *) fail "no qualified release for operating system: $system" ;;
esac
case "$machine" in x86_64|aarch64|arm64) ;; *) fail "no qualified release for architecture: $machine" ;; esac
command -v gh >/dev/null 2>&1 || fail 'install gh with attestation verification support separately; no automatic dependency install'
if [ -z "$archive$rollback" ]; then
    command -v curl >/dev/null 2>&1 || fail 'install curl separately; no automatic dependency install'
fi

if [ -n "$helper" ]; then
    [ -f "$helper" ] && [ -x "$helper" ] && [ ! -L "$helper" ] || fail '--helper must be an ordinary executable, not a symlink'
    if [ -n "$rollback" ]; then
        set -- rollback --version "$version" --prefix "$prefix"
        if [ -n "$bundle" ]; then set -- "$@" --bundle "$bundle"; fi
        exec "$helper" "$@"
    fi
    set -- install --version "$version" --commit "$commit" --prefix "$prefix"
    if [ -n "$archive" ]; then set -- "$@" --archive "$archive"; fi
    if [ -n "$bundle" ]; then set -- "$@" --bundle "$bundle"; fi
    exec "$helper" "$@"
fi

printf '%s\n' \
    "kk installer: BLOCKED for $system/$machine, requested v$version at $commit." \
    'A bounded native helper is required to read the release manifest, freeze archive bytes,' \
    'verify GitHub provenance with gh, validate every member, and atomically activate.' \
    'Pass --helper ABSOLUTE_ALREADY_TRUSTED_KK. No helper is implicitly trusted.' \
    'Nothing was downloaded, extracted, installed, deleted, or changed.' >&2
exit 3
