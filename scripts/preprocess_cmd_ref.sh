#!/usr/bin/env bash
#
# SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
#
# SPDX-License-Identifier: EUPL-1.2

# Use: preprocess_cmd_ref.sh <output-dir>
#

set -Eeuo pipefail
# IFS=$'\n\t'

on_error() {
    local exit_code=$?
    local line=$1
    echo "Error on line $line (exit code $exit_code)" >&2
    exit "$exit_code"
}

function download () {
    readonly curl="/usr/bin/curl"

    readonly base_url="https://repo.lauterbach.com/pdf"
    declare -a -r curls_opts=("--silent" "--show-error" "--remove-on-error" "--remote-name")

    # Command References
    declare -a -r cmd_refs=(a b c d e f g h i j k l m n o p q r s t u v w x y z)
    printf "$base_url/general_ref_%s.pdf\0" "${cmd_refs[@]}" | xargs --null --max-args=1 --max-procs=26 -- \
        $curl "${curls_opts[@]}" --output-dir "$1"

    # Function references & misc
    declare -a -r cmd_misc=(commandlist ide_func general_func)
    printf "$base_url/%s.pdf\0" "${cmd_misc[@]}" | xargs --null --max-args=1 --max-procs=0 -- \
        $curl "${curls_opts[@]}" --output-dir "$1"
}

function convert () {
    readonly pdftotext="/usr/bin/pdftotext"

    declare -a -r pdftotext_opts_man=("-layout" "-x" "0" "-y" "0" "-W" "4000" "-H" "675")

    find "$1" -type f -name "*.pdf" -print0 | xargs --null --max-args=1 --max-procs=0 --replace=IN -- \
        $pdftotext "${pdftotext_opts_man[@]}" IN IN.man.txt

    declare -a -r pdftotext_opts_copyright=("-layout" "-x" "0" "-y" "675" "-W" "4000" "-H" "1000")

    find "$1" -type f -name "*.pdf" -print0 | xargs --null --max-args=1 --max-procs=0 --replace=IN -- \
        $pdftotext "${pdftotext_opts_copyright[@]}" IN IN.copyright.txt
}

trap 'on_error $LINENO' ERR

if [[ ! -d "$1" ]]; then
    echo "Directory \"$1\" does not exist. Exiting..."
    exit 1
fi

download "$1"
convert "$1"
