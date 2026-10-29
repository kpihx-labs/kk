#!/bin/sh
# Verified raw-helper bootstrap, never an archive extractor.
set -eu

fail() { printf '%s\n' "kk installer: $*" >&2; exit 1; }
usage() {
    printf '%s\n' \
        'Usage: sh install.sh --version VERSION --commit COMMIT [--prefix ABSOLUTE_PATH]' \
        '                     [--archive ABSOLUTE_PATH [--bundle ABSOLUTE_PATH]]' \
        '                     [--trusted-root ABSOLUTE_PATH (requires --bundle and --helper)]' \
        '                     [--helper ABSOLUTE_ALREADY_TRUSTED_KK] [--rollback]' \
        'Requires separately installed capable gh, curl, jq, sha256sum, timeout, and native ldconfig.' \
        'Without --helper, Linux downloads and verifies the exact release helper AND notices.' \
        'The helper owns download, gh verification, bounded validation and activation.' \
        'Local archives must pass the same provenance/content gates. Profiles are untouched.'
}

version='' commit='' archive='' bundle='' trusted_root='' helper='' rollback='' prefix=${HOME:?HOME must be set}/.local
seen=' '
while [ "$#" -gt 0 ]; do
    key=$1
    case "$key" in
        --help|-h) usage; exit 0 ;;
        --rollback)
            [ -z "$rollback" ] || fail 'duplicate --rollback'
            rollback=yes
            shift ;;
        --version|--commit|--prefix|--archive|--bundle|--trusted-root|--helper)
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
                --trusted-root) trusted_root=$2 ;;
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
for path in "$prefix" "${archive:-/}" "${bundle:-/}" "${trusted_root:-/}" "${helper:-/}"; do
    case "$path" in /*) ;; *) fail 'paths must be absolute' ;; esac
done
[ -z "$bundle" ] || [ -n "$archive$rollback" ] || fail '--bundle requires --archive or --rollback'
if [ -n "$trusted_root" ]; then
    [ -n "$bundle" ] || fail '--trusted-root requires --bundle'
    [ -n "$helper" ] || fail '--trusted-root requires an explicitly trusted --helper; offline mode never downloads a helper'
fi

system=$(uname -s) || fail 'cannot detect operating system'
machine=$(uname -m) || fail 'cannot detect architecture'
case "$system" in
    Linux) ;;
    MINGW*|MSYS*|CYGWIN*|Windows*) fail 'native Windows release is not shipped; refusing installation' ;;
    *) fail "no qualified release for operating system: $system" ;;
esac
case "$machine" in
    x86_64) target=linux-x86_64 ;;
    aarch64|arm64) target=linux-aarch64 ;;
    *) fail "no qualified release for architecture: $machine" ;;
esac
command -v gh >/dev/null 2>&1 || fail 'install gh with attestation verification support separately; no automatic dependency install'
if [ -z "$helper" ] || [ -z "$archive$rollback" ]; then
    command -v curl >/dev/null 2>&1 || fail 'install curl separately; no automatic dependency install'
fi

if [ -z "$helper" ]; then
    [ -z "$rollback" ] || fail '--rollback requires an explicitly trusted --helper'
    command -v timeout >/dev/null 2>&1 || fail 'install timeout separately; no automatic dependency install'
    command -v jq >/dev/null 2>&1 || fail 'install jq separately; no automatic dependency install'
    command -v sha256sum >/dev/null 2>&1 || fail 'install sha256sum separately; no automatic dependency install'
    # Retained private distribution, not a managed installation or a profile. No cleanup
    # races: callers may inspect/retain the companion even if installation later fails.
    umask 077
    work=$(mktemp -d "${TMPDIR:-/tmp}/kk-bootstrap.XXXXXXXXXX") || fail 'cannot create private distribution directory'
    printf '%s\n' "kk bootstrap distribution retained at: $work" >&2
    unset GH_DEBUG
    export GH_HOST=github.com
    (ulimit -f 256; timeout -k 5 30 gh attestation verify --help > "$work/gh-help") || fail 'gh attestation capability check failed'
    for flag in --repo --signer-workflow --source-ref --source-digest --deny-self-hosted-runners; do
        grep -F -q -- "$flag" "$work/gh-help" || fail "gh lacks required policy flag: $flag"
    done
    helper_name="kk-v$version-$target-bootstrap.bin"
    notices_name="kk-v$version-$target-bootstrap-notices.json"
    # Validate the whole authority, not a hostname suffix. No URL or signed query
    # is logged. Relative Location values are resolved by curl, never evaluated.
    allowed_url() {
        [ "${#1}" -le 8192 ] || return 1
        case "$1" in *[!\ -~]*) return 1 ;; esac
        case "$1" in
            https://github.com/*|https://release-assets.githubusercontent.com/*|https://objects.githubusercontent.com/*) return 0 ;;
            *) return 1 ;;
        esac
    }
    for asset in "$helper_name" "$notices_name"; do
        url="https://github.com/kpihx-labs/kk/releases/download/v$version/$asset"
        hops=0
        while :; do
            allowed_url "$url" || fail 'refused bootstrap redirect authority or URL'
            # No --location: authorize the next resolved URL BEFORE any connection.
            # Body and metadata have OS ceilings as well as post-download limits.
            (ulimit -f 131072; timeout -k 5 125 curl --disable --silent \
                --max-redirs 0 --proto '=https' --proto-redir '=https' \
                --connect-timeout 15 --max-time 120 --max-filesize 134217728 \
                --output "$work/$asset" --write-out '%{http_code}\n%{redirect_url}\n' \
                "$url" > "$work/response") || fail "download failed: $asset"
            metadata_size=$(wc -c < "$work/response") || fail 'cannot measure response metadata'
            [ "$metadata_size" -le 8192 ] || fail 'oversized redirect metadata'
            {
                IFS= read -r status || fail 'invalid response status'
                IFS= read -r next_url || fail 'invalid response redirect'
                if IFS= read -r extra || [ -n "$extra" ]; then fail 'extra response metadata'; fi
            } < "$work/response"
            case "$status" in
                200) [ -z "$next_url" ] || fail 'unexpected final redirect'; break ;;
                301|302|303|307|308)
                    [ "$hops" -lt 5 ] || fail 'too many bootstrap redirects'
                    allowed_url "$next_url" || fail 'refused bootstrap redirect authority or URL'
                    url=$next_url
                    hops=$((hops + 1)) ;;
                *) fail 'unexpected bootstrap HTTP status' ;;
            esac
        done
        size=$(wc -c < "$work/$asset") || fail 'cannot measure download'
        [ "$size" -gt 0 ] && [ "$size" -le 134217728 ] || fail 'empty or oversized bootstrap asset'
        timeout -k 5 120 gh attestation verify "$work/$asset" --repo kpihx-labs/kk \
            --signer-workflow kpihx-labs/kk/.github/workflows/check.yml \
            --source-ref "refs/tags/v$version" --source-digest "$commit" \
            --deny-self-hosted-runners || fail "provenance verification failed: $asset"
    done
    # Authenticating two subjects independently does not bind their bytes together.
    # Parse only the authenticated record, then match every release selector.
    helper="$work/$helper_name"
    architecture=${target#linux-}
    helper_digest=$(sha256sum < "$helper") || fail 'cannot hash helper'
    helper_digest=${helper_digest%% *}
    # jq variables are deliberately interpreted by jq, not the shell.
    # shellcheck disable=SC2016
    (ulimit -f 256; timeout -k 5 30 jq -e -s \
        --arg version "$version" --arg commit "$commit" \
        --arg target "$architecture-unknown-linux-gnu" --arg arch "$architecture" \
        --arg helper "$helper_name" --arg notices "$notices_name" --arg sha "$helper_digest" '
        length == 1 and (.[0] | .format == 1 and .version == $version and
        .commit == $commit and .target == $target and .platform == "linux" and
        .architecture == $arch and .executable_format == "elf" and .suffix == ".bin" and
        .notices_asset == $notices and .helper.asset == $helper and .helper.sha256 == $sha)
        ' "$work/$notices_name" > "$work/binding-result") || fail 'authenticated bootstrap companion binding mismatch'
    # Both subjects and their association must authenticate BEFORE chmod/execution.
    chmod 500 "$helper" || fail 'cannot enable verified helper'
    printf '%s\n' "Verified bound notice inventory: $work/$notices_name" >&2
fi

if [ -n "$helper" ]; then
    [ -f "$helper" ] && [ -x "$helper" ] && [ ! -L "$helper" ] || fail '--helper must be an ordinary executable, not a symlink'
    if [ -n "$rollback" ]; then
        set -- rollback --version "$version" --prefix "$prefix"
        if [ -n "$bundle" ]; then set -- "$@" --bundle "$bundle"; fi
        if [ -n "$trusted_root" ]; then set -- "$@" --trusted-root "$trusted_root"; fi
        exec "$helper" "$@"
    fi
    set -- install --version "$version" --commit "$commit" --prefix "$prefix"
    if [ -n "$archive" ]; then set -- "$@" --archive "$archive"; fi
    if [ -n "$bundle" ]; then set -- "$@" --bundle "$bundle"; fi
    if [ -n "$trusted_root" ]; then set -- "$@" --trusted-root "$trusted_root"; fi
    exec "$helper" "$@"
fi
