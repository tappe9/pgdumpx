#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

fail() {
    echo "peak RSS measurement failed: $*" >&2
    exit 1
}

[[ $# -ge 4 ]] || fail "usage: $0 <OUTPUT_TSV> <LABEL> -- <COMMAND> [ARGS...]"
readonly OUTPUT="$1"
readonly LABEL="$2"
shift 2
[[ "$1" == "--" ]] || fail "expected -- before command"
shift
[[ $# -ge 1 ]] || fail "missing command"

TIME_COMMAND=""
for candidate in gtime /usr/bin/time; do
    if command -v "${candidate}" >/dev/null 2>&1 \
        && "${candidate}" --version 2>/dev/null | grep -q 'GNU Time'; then
        TIME_COMMAND="$(command -v "${candidate}")"
        break
    fi
done
[[ -n "${TIME_COMMAND}" ]] || fail "GNU time is required (install 'time' on Linux or 'gnu-time' on macOS)"

mkdir -p "$(dirname "${OUTPUT}")"
if [[ ! -e "${OUTPUT}" ]]; then
    printf 'label\tpeak_rss_kib\tmeasurement_tool\n' > "${OUTPUT}"
fi

tmp_rss="$(mktemp)"
cleanup() {
    rm -f "${tmp_rss}"
}
trap cleanup EXIT

set +e
"${TIME_COMMAND}" -f '%M' -o "${tmp_rss}" "$@" >/dev/null
status=$?
set -e
(( status == 0 )) || fail "measured command exited with status ${status}"

peak_rss_kib="$(tr -d '[:space:]' < "${tmp_rss}")"
case "${peak_rss_kib}" in
    ''|*[!0-9]*) fail "unexpected GNU time RSS output: ${peak_rss_kib}" ;;
esac

printf '%s\t%s\tGNU time %%M (KiB)\n' "${LABEL}" "${peak_rss_kib}" >> "${OUTPUT}"
