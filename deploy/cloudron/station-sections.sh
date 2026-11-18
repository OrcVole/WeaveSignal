#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# The control plane's [studio] and [steering] sections, written from weavesignal.env (issue 5).
# start.sh sources this file and appends `station_sections` to control.toml; the tests source it
# too. A setting left empty is left out of the file, so the control plane's own default applies
# and an unset setting changes nothing; with none set, nothing is written at all.
#
# The settings (every one optional):
#   STUDIO_SHOW                the show the desk heads: plain text (quotes and backslashes are escaped)
#   STUDIO_LISTENER_DELAY_MS   how far behind listeners hear, in ms (default 13000)
#   STUDIO_RUNDOWN, STUDIO_BANNED, STEERING_EDGES
#                              TOML inline tables, comma-separated, in single quotes in the file
#   STEERING_TTL_S, STEERING_CHECK_EVERY_S, STEERING_CHECK_TIMEOUT_MS, STEERING_DOWN_AFTER
#                              whole numbers (the control plane checks their ranges)
#   STEERING_CLONE_FROM        the pathway edges with a host are cloned from (default ".")
#
# /app/data/station.toml, when it exists, still replaces a whole section of these.

declare -F log >/dev/null || log() { echo "weavesignal-start: $*"; }

# A TOML basic string: backslashes and quotes escaped, control characters refused.
station_toml_string() { # name value
    local v=$2
    [[ $v != *[[:cntrl:]]* ]] || { log "$1 in weavesignal.env holds a control character"; return 1; }
    v=${v//\\/\\\\}
    v=${v//\"/\\\"}
    printf '"%s"' "$v"
}

station_sections() {
    local n s studio='' steering=''
    # Whole numbers, written as they are.
    for n in STUDIO_LISTENER_DELAY_MS STEERING_TTL_S STEERING_CHECK_EVERY_S STEERING_CHECK_TIMEOUT_MS STEERING_DOWN_AFTER; do
        [[ -z ${!n:-} || ${!n} =~ ^[0-9]+$ ]] || { log "$n=${!n} in weavesignal.env is not a whole number"; return 1; }
    done
    if [[ -n ${STUDIO_SHOW:-} ]]; then
        s=$(station_toml_string STUDIO_SHOW "$STUDIO_SHOW") || return 1
        studio+="show = $s"$'\n'
    fi
    [[ -z ${STUDIO_LISTENER_DELAY_MS:-} ]] || studio+="listener_delay_ms = $STUDIO_LISTENER_DELAY_MS"$'\n'
    [[ -z ${STUDIO_RUNDOWN:-} ]] || studio+="rundown = [$STUDIO_RUNDOWN]"$'\n'
    [[ -z ${STUDIO_BANNED:-} ]] || studio+="banned = [$STUDIO_BANNED]"$'\n'
    [[ -z ${STEERING_TTL_S:-} ]] || steering+="ttl_s = $STEERING_TTL_S"$'\n'
    [[ -z ${STEERING_CHECK_EVERY_S:-} ]] || steering+="check_every_s = $STEERING_CHECK_EVERY_S"$'\n'
    [[ -z ${STEERING_CHECK_TIMEOUT_MS:-} ]] || steering+="check_timeout_ms = $STEERING_CHECK_TIMEOUT_MS"$'\n'
    [[ -z ${STEERING_DOWN_AFTER:-} ]] || steering+="down_after = $STEERING_DOWN_AFTER"$'\n'
    if [[ -n ${STEERING_CLONE_FROM:-} ]]; then
        s=$(station_toml_string STEERING_CLONE_FROM "$STEERING_CLONE_FROM") || return 1
        steering+="clone_from = $s"$'\n'
    fi
    [[ -z ${STEERING_EDGES:-} ]] || steering+="edges = [$STEERING_EDGES]"$'\n'
    if [[ -n $studio ]]; then
        printf '\n# From STUDIO_* in weavesignal.env (/app/data/station.toml replaces this section).\n[studio]\n%s' "$studio"
    fi
    if [[ -n $steering ]]; then
        printf '\n# From STEERING_* in weavesignal.env (/app/data/station.toml replaces this section).\n[steering]\n%s' "$steering"
    fi
    return 0
}
