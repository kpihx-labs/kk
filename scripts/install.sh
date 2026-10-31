#!/bin/sh
# Linux HTTPS/checksum install, with an explicit advanced Sigstore bootstrap.
set -eu

fail() { printf '%s\n' "kk installer: $*" >&2; exit 1; }
usage() {
    printf '%s\n' \
        'Simple Linux: curl --proto "=https" --tlsv1.2 -fsS https://github.com/kpihx-labs/kk/releases/download/v1.0.0/install.sh | sh' \
        'No arguments installs stable 1.0.0; --version VERSION and --prefix PATH are optional.' \
        'Simple mode needs curl and sha256sum, not gh/jq; it verifies HTTPS release checksums,' \
        'not Sigstore provenance. No sudo, services, profile changes or downloaded-code execution.' \
        'Supplying --commit selects advanced verification (no automatic fallback):' \
        'Usage: sh install.sh --version VERSION --commit COMMIT [--prefix ABSOLUTE_PATH]' \
        '                     [--archive ABSOLUTE_PATH [--bundle ABSOLUTE_PATH]]' \
        '                     [--trusted-root ABSOLUTE_PATH (requires --bundle and --helper or --bootstrap-dir)]' \
        '                     [--helper ABSOLUTE_ALREADY_TRUSTED_KK] [--rollback]' \
        '                     [--bootstrap-dir ABSOLUTE_SIGNED_LOCAL_ASSET_DIRECTORY]' \
        '                     [--verify-only (authenticate/validate, never activate)]' \
        'Requires separately installed capable gh; bootstrap also needs jq, curl (online),' \
        'timeout/gtimeout, and sha256sum or shasum. Native ABI tools are platform-specific.' \
        'Without --helper, Linux/macOS download and verify the exact release helper AND notices.' \
        '--bootstrap-dir instead verifies signed local helper/notices plus ASSET.jsonl bundles; no download.' \
        'The helper owns download, gh verification, bounded validation and activation.' \
        'Local archives must pass the same provenance/content gates. Profiles are untouched.'
}

version='1.0.0' commit='' archive='' bundle='' trusted_root='' helper='' bootstrap_dir='' rollback='' verify_only='' prefix=${HOME:?HOME must be set}/.local
seen=' '
while [ "$#" -gt 0 ]; do
    key=$1
    case "$key" in
        --help|-h) usage; exit 0 ;;
        --verify-only)
            [ -z "$verify_only" ] || fail 'duplicate --verify-only'
            verify_only=yes
            shift ;;
        --rollback)
            [ -z "$rollback" ] || fail 'duplicate --rollback'
            rollback=yes
            shift ;;
        --version|--commit|--prefix|--archive|--bundle|--trusted-root|--helper|--bootstrap-dir)
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
                --bootstrap-dir) bootstrap_dir=$2 ;;
            esac
            shift 2 ;;
        *) fail "unknown argument: $key" ;;
    esac
done

# The simple path deliberately never invokes the downloaded helper: its native
# installer requires Sigstore. HTTPS release checksums are a weaker, explicit
# trust tier, not a fallback when an advanced verifier fails.
simple_install() {
    [ "$(uname -s)" = Linux ] || fail 'simple installation supports Linux only; use the advanced path for macOS'
    case "$(uname -m)" in
        x86_64) simple_target=linux-x86_64 ;;
        aarch64|arm64) simple_target=linux-aarch64 ;;
        *) fail 'unsupported Linux architecture' ;;
    esac
    for tool in curl sha256sum mktemp wc cp chmod mkdir ln; do
        command -v "$tool" >/dev/null 2>&1 || fail "install $tool separately"
    done
    case "$prefix" in /*) ;; *) fail 'paths must be absolute' ;; esac
    case "$prefix/" in *'/../'*|*'/./'*|*'//'*) fail 'prefix must be a canonical absolute path' ;; esac
    # Never traverse a symlink into an unrelated installation tree.
    probe=$prefix/lib/kk-simple/$version
    while [ "$probe" != / ]; do
        [ ! -L "$probe" ] || fail 'installation paths must not contain symlinks'
        probe=${probe%/*}; [ -n "$probe" ] || probe=/
    done
    [ ! -L "$prefix/bin" ] || fail 'bin must not be a symlink'
    [ ! -e "$prefix/bin/kk" ] && [ ! -L "$prefix/bin/kk" ] || fail 'refusing to replace an existing kk command'
    [ ! -e "$prefix/lib/kk-simple/$version" ] || fail 'version directory already exists; inspect it before retrying'
    umask 077
    work=$(mktemp -d "${TMPDIR:-/tmp}/kk-simple.XXXXXXXXXX") || fail 'cannot create private download directory'
    printf '%s\n' "HTTPS/checksum trust only (no Sigstore). Downloads retained at: $work" >&2
    simple_download() {
        url="https://github.com/kpihx-labs/kk/releases/download/v$version/$1"
        hops=0
        while :; do
            case "$url" in
                https://github.com/*|https://release-assets.githubusercontent.com/*|https://objects.githubusercontent.com/*) ;;
                *) fail 'refused download redirect authority' ;;
            esac
            [ "${#url}" -le 8192 ] || fail 'oversized redirect'
            case "$url" in *[!\ -~]*) fail 'invalid redirect' ;; esac
            (ulimit -f "$3"; curl --disable --silent --show-error --proto '=https' --proto-redir '=https' \
                --tlsv1.2 --max-redirs 0 --connect-timeout 15 --max-time 120 --max-filesize "$2" \
                --output "$work/$1" --write-out '%{http_code}\n%{redirect_url}\n' "$url" > "$work/response") || fail 'download failed'
            [ "$(wc -c < "$work/response")" -le 8192 ] || fail 'oversized response metadata'
            {
                IFS= read -r status || fail 'missing response status'
                IFS= read -r next_url || fail 'missing response redirect'
                if IFS= read -r extra || [ -n "$extra" ]; then fail 'extra response metadata'; fi
            } < "$work/response"
            case "$status" in
                200) [ -z "$next_url" ] || fail 'unexpected redirect'; break ;;
                301|302|303|307|308) [ "$hops" -lt 5 ] || fail 'too many redirects'
                    url=$next_url; hops=$((hops + 1)) ;;
                *) fail 'unexpected download HTTP status' ;;
            esac
        done
        size=$(wc -c < "$work/$1")
        [ "$size" -gt 0 ] && [ "$size" -le "$2" ] || fail 'empty or oversized download'
    }
    stem="kk-v$version-$simple_target"
    simple_download "$stem.sha256" 65536 128
    for asset in "$stem-bootstrap.bin" "$stem-bootstrap-notices.json"; do
        expected='' matches=0
        while IFS=' ' read -r checksum name; do
            if [ "$name" = "$asset" ]; then expected=$checksum; matches=$((matches + 1)); fi
        done < "$work/$stem.sha256"
        [ "$matches" -eq 1 ] || fail 'missing or duplicate release checksum'
        case "$expected" in *[!0-9a-f]*|'') fail 'invalid SHA-256' ;; esac
        [ "${#expected}" -eq 64 ] || fail 'invalid SHA-256 length'
        simple_download "$asset" 134217728 262144
        actual=$(sha256sum < "$work/$asset") || fail 'cannot hash download'
        [ "${actual%% *}" = "$expected" ] || fail 'release checksum mismatch'
    done
    # Only now create installation state. Exclusive directory/link creation refuses
    # collisions; failure may retain a partial version, never activates that version.
    directory=$prefix/lib/kk-simple/$version
    mkdir -p "$prefix/lib/kk-simple" "$prefix/bin" || fail 'cannot create user installation parents'
    mkdir "$directory" || fail 'cannot exclusively create version directory'
    cp "$work/$stem-bootstrap.bin" "$directory/kk" || fail 'cannot stage binary'
    cp "$work/$stem-bootstrap-notices.json" "$directory/notices.json" || fail 'cannot preserve licenses'
    cp "$work/$stem.sha256" "$directory/release.sha256" || fail 'cannot preserve checksums'
    chmod 500 "$directory/kk" || fail 'cannot enable binary'
    ln -s "$directory/kk" "$prefix/bin/kk" || fail 'cannot activate without replacing an existing command'
    printf '%s\n' "Installed K&K $version at $prefix/bin/kk" \
        "Keep $directory/notices.json with the binary (project and upstream licenses)." \
        "Add $prefix/bin to PATH if necessary, then run: kk ui"
}
if [ -n "$verify_only" ]; then
    [ -z "$rollback" ] || fail '--verify-only excludes --rollback'
    [ -z "$helper" ] || fail '--verify-only authenticates helper/notices; use online bootstrap or --bootstrap-dir, not --helper'
fi

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
if [ -z "$commit$archive$bundle$trusted_root$helper$bootstrap_dir$rollback$verify_only" ]; then
    simple_install
    exit 0
fi
if [ -z "$rollback" ] || [ -n "$bootstrap_dir" ]; then
    case "$commit" in ''|*[!0-9a-f]*) fail 'commit must be a full lowercase 40-character SHA' ;; esac
    [ "${#commit}" -eq 40 ] || fail 'commit must be a full lowercase 40-character SHA'
else
    [ -z "$commit$archive" ] || fail '--rollback does not accept commit or archive'
fi
[ -z "$rollback" ] || [ -z "$archive" ] || fail '--rollback uses its retained archive, not --archive'
for path in "$prefix" "${archive:-/}" "${bundle:-/}" "${trusted_root:-/}" "${helper:-/}" "${bootstrap_dir:-/}"; do
    case "$path" in /*) ;; *) fail 'paths must be absolute' ;; esac
done
[ -z "$bundle" ] || [ -n "$archive$rollback" ] || fail '--bundle requires --archive or --rollback'
if [ -n "$bootstrap_dir" ]; then
    [ -z "$helper" ] || fail '--bootstrap-dir excludes --helper'
    [ -n "$archive$rollback" ] && [ -n "$bundle" ] && [ -n "$trusted_root" ] || fail '--bootstrap-dir requires --archive or --rollback, plus local --bundle and --trusted-root'
    [ -d "$bootstrap_dir" ] && [ ! -L "$bootstrap_dir" ] || fail '--bootstrap-dir must be an ordinary directory'
fi
if [ -n "$trusted_root" ]; then
    [ -n "$bundle" ] || fail '--trusted-root requires --bundle'
    [ -n "$helper$bootstrap_dir" ] || fail '--trusted-root requires an explicitly trusted --helper or signed local --bootstrap-dir; offline mode never downloads a helper'
fi

system=$(uname -s) || fail 'cannot detect operating system'
machine=$(uname -m) || fail 'cannot detect architecture'
case "$system" in
    Linux) platform=linux; abi=unknown-linux-gnu; executable_format=elf ;;
    Darwin) platform=macos; abi=apple-darwin; executable_format=macho ;;
    MINGW*|MSYS*|CYGWIN*|Windows*) fail 'native Windows release is not shipped; refusing installation' ;;
    *) fail "no qualified release for operating system: $system" ;;
esac
case "$machine" in
    x86_64) target=$platform-x86_64 ;;
    aarch64|arm64) target=$platform-aarch64 ;;
    *) fail "no qualified release for architecture: $machine" ;;
esac
command -v gh >/dev/null 2>&1 || fail 'install gh with attestation verification support separately; no automatic dependency install'
if { [ -z "$helper" ] && [ -z "$bootstrap_dir" ]; } || [ -z "$archive$rollback" ]; then
    command -v curl >/dev/null 2>&1 || fail 'install curl separately; no automatic dependency install'
fi

if [ -z "$helper" ]; then
    [ -z "$rollback" ] || [ -n "$bootstrap_dir" ] || fail '--rollback requires a trusted --helper or signed local --bootstrap-dir'
    timer=$(command -v timeout || command -v gtimeout) || fail 'install timeout/gtimeout separately; no automatic dependency install'
    command -v jq >/dev/null 2>&1 || fail 'install jq separately; no automatic dependency install'
    if command -v sha256sum >/dev/null 2>&1; then
        hash=sha256sum
    else
        command -v shasum >/dev/null 2>&1 || fail 'install sha256sum or shasum separately; no automatic dependency install'
        hash=shasum
    fi
    # Retained private distribution, not a managed installation or a profile. No cleanup
    # races: callers may inspect/retain the companion even if installation later fails.
    umask 077
    work=$(mktemp -d "${TMPDIR:-/tmp}/kk-bootstrap.XXXXXXXXXX") || fail 'cannot create private distribution directory'
    printf '%s\n' "kk bootstrap distribution retained at: $work" >&2
    unset GH_DEBUG
    export GH_HOST=github.com
    export GH_NO_UPDATE_NOTIFIER=1
    (ulimit -f 256; "$timer" -k 5 30 gh attestation verify --help > "$work/gh-help") || fail 'gh attestation capability check failed'
    flags='--repo --signer-workflow --source-ref --source-digest --deny-self-hosted-runners'
    if [ -n "$bootstrap_dir" ]; then flags="$flags --bundle --custom-trusted-root"; fi
    for flag in $flags; do
        grep -F -q -- "$flag" "$work/gh-help" || fail "gh lacks required policy flag: $flag"
    done
    helper_name="kk-v$version-$target-bootstrap.bin"
    notices_name="kk-v$version-$target-bootstrap-notices.json"
    # Snapshot all local verifier inputs into the private distribution before any
    # authentication. Never chmod/execute a removable-media pathname. Bounds also
    # cover races replacing a source file after the ordinary-file check.
    snapshot() {
        [ -f "$1" ] && [ ! -L "$1" ] || fail 'local bootstrap inputs must be ordinary files'
        (umask 177; ulimit -f "$3"; "$timer" -k 5 30 cp "$1" "$2") || fail 'local bootstrap snapshot failed'
        size=$(wc -c < "$2") || fail 'cannot measure local bootstrap input'
        [ "$size" -gt 0 ] && [ "$size" -le "$4" ] || fail 'empty or oversized local bootstrap input'
    }
    if [ -n "$bootstrap_dir" ]; then
        snapshot "$trusted_root" "$work/trusted-root.jsonl" 2048 1048576
        trusted_root="$work/trusted-root.jsonl"
        for asset in "$helper_name" "$notices_name"; do
            snapshot "$bootstrap_dir/$asset" "$work/$asset" 262144 134217728
            snapshot "$bootstrap_dir/$asset.jsonl" "$work/$asset.jsonl" 32768 16777216
        done
    fi
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
        if [ -z "$bootstrap_dir" ]; then
        url="https://github.com/kpihx-labs/kk/releases/download/v$version/$asset"
        hops=0
        while :; do
            allowed_url "$url" || fail 'refused bootstrap redirect authority or URL'
            # No --location: authorize the next resolved URL BEFORE any connection.
            # Body and metadata have OS ceilings as well as post-download limits.
            (ulimit -f 131072; "$timer" -k 5 125 curl --disable --silent \
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
        fi
        size=$(wc -c < "$work/$asset") || fail 'cannot measure download'
        [ "$size" -gt 0 ] && [ "$size" -le 134217728 ] || fail 'empty or oversized bootstrap asset'
        set -- attestation verify "$work/$asset" --repo kpihx-labs/kk \
            --signer-workflow kpihx-labs/kk/.github/workflows/check.yml \
            --source-ref "refs/tags/v$version" --source-digest "$commit" --deny-self-hosted-runners
        if [ -n "$bootstrap_dir" ]; then
            set -- "$@" --bundle "$work/$asset.jsonl" --custom-trusted-root "$trusted_root"
        fi
        "$timer" -k 5 120 gh "$@" || fail "provenance verification failed: $asset"
    done
    # Authenticating two subjects independently does not bind their bytes together.
    # Parse only the authenticated record, then match every release selector.
    helper="$work/$helper_name"
    architecture=${target#*-}
    if [ "$hash" = shasum ]; then
        helper_digest=$(shasum -a 256 < "$helper") || fail 'cannot hash helper'
    else
        helper_digest=$(sha256sum < "$helper") || fail 'cannot hash helper'
    fi
    helper_digest=${helper_digest%% *}
    # jq variables are deliberately interpreted by jq, not the shell.
    # shellcheck disable=SC2016
    (ulimit -f 256; "$timer" -k 5 30 jq -e -s \
        --arg version "$version" --arg commit "$commit" \
        --arg target "$architecture-$abi" --arg arch "$architecture" --arg platform "$platform" --arg format "$executable_format" \
        --arg helper "$helper_name" --arg notices "$notices_name" --arg sha "$helper_digest" '
        length == 1 and (.[0] | .format == 1 and .version == $version and
        .commit == $commit and .target == $target and .platform == $platform and
        .architecture == $arch and .executable_format == $format and .suffix == ".bin" and
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
    # Native verification owns archive/manifest/ABI binding without entering the
    # managed transaction. Older helpers must refuse; never retry as installation.
    if [ -n "$verify_only" ]; then set -- "$@" --verify-only; fi
    if [ -n "$archive" ]; then set -- "$@" --archive "$archive"; fi
    if [ -n "$bundle" ]; then set -- "$@" --bundle "$bundle"; fi
    if [ -n "$trusted_root" ]; then set -- "$@" --trusted-root "$trusted_root"; fi
    exec "$helper" "$@"
fi
