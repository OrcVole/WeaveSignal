# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Sourced by the tests that run the Compose bundle through `podman compose` directly (the
# justfile does the same for every recipe, and weavesignal.sh for itself). The bundle gives each
# container oom_score_adj ${WEAVESIGNAL_OOM_SCORE_ADJ:-0}; a session that runs above 0 cannot
# `podman exec` into a container made with 0 ("crun: write to /proc/self/oom_score_adj:
# Permission denied"), so PostgreSQL's healthcheck never passes. Sets the variable to the
# session's own value when that is above 0 and it is not set already; never lowers anything.
if [ -z "${WEAVESIGNAL_OOM_SCORE_ADJ:-}" ]; then
  session_oom=$(cat /proc/self/oom_score_adj 2>/dev/null || echo 0)
  case $session_oom in
    ''|*[!0-9]*|0) ;;
    *) export WEAVESIGNAL_OOM_SCORE_ADJ=$session_oom ;;
  esac
  # The rehearsal desk (1.3) runs at 500, or the session's own when that is higher.
  case $session_oom in
    ''|*[!0-9]*) ;;
    *) [ "$session_oom" -le 500 ] || export WEAVESIGNAL_REHEARSAL_OOM_SCORE_ADJ=$session_oom ;;
  esac
  unset session_oom
fi
