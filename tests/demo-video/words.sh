# SPDX-License-Identifier: AGPL-3.0-only
# SPDX-FileCopyrightText: 2026 The WeaveSignal contributors
#
# Comparing what Whisper heard with the script, for synth.sh and check.sh (sourced; needs
# $script, the path of script.json).

# Words for comparing: lower case, no punctuation, each name's spellings folded to one.
norm() {
  jq -rn --arg t "$1" --arg q "'" --slurpfile s "$script" '
    ($s[0].heard_as | to_entries) as $names
    | $t | ascii_downcase | gsub("[‘’" + $q + "]"; "") | gsub("-"; " ")
    | gsub("[^a-z0-9 ]"; " ")
    | reduce $names[] as $n (.; reduce ($n.value[] | gsub("-"; " ") | gsub($q; "")) as $alt (.; gsub("\\b" + $alt + "\\b"; ($n.key | ascii_downcase | gsub("-"; " ")))))
    | gsub("nineteen fifty six"; "1956") | gsub("\\b(ten|10) past eleven\\b"; "1110")
    | [splits(" +")] | map(select(length > 0)) | join(" ")'
}
# Word edit distance between two normalised strings.
distance() {
  jq -rn --arg a "$1" --arg b "$2" '
    ($a | split(" ")) as $x | ($b | split(" ")) as $y
    | reduce range(0; $x | length) as $i ([range(0; ($y | length) + 1)];
        . as $prev
        | reduce range(0; $y | length) as $j ([$i + 1];
            . + [[$prev[$j + 1] + 1, .[$j] + 1, $prev[$j] + (if $x[$i] == $y[$j] then 0 else 1 end)] | min]))
    | .[-1]'
}
