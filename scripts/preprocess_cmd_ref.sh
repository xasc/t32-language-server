#!/usr/bin/env bash
#
# SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
#
# SPDX-License-Identifier: EUPL-1.2

# Use: preprocess_cmd_ref.sh <output-dir>
#

set -Eeuo pipefail
set -x

readonly base_url="https://repo.lauterbach.com/pdf"
readonly curl="/usr/bin/curl"

declare -a -r curl_opts=(--silent --show-error --remove-on-error --remote-name)

on_error () {
    local exit_code=$?
    local line=$1
    echo "Error on line $line (exit code $exit_code)" >&2
    exit "$exit_code"
}

function usage () {
    echo "Usage: $0 [-c] [-f] dir"
    echo ""
    echo "Options:"
    echo "  -f    Include information about PRACTICE functions"
    echo "  -c    Include information about TRACE32® commands"

    exit 1
}

function download_functions () {
    # Function references
    declare -a -r cmd_misc=(ide_func general_func)
    printf "$base_url/%s.pdf\0" "${cmd_misc[@]}" | xargs --null --max-args=1 --max-procs=0 -- \
        $curl "${curl_opts[@]}" --output-dir "$1"
}

function download_commands () {
    # Command references
    declare -a -r cmd_refs=(a b c d e f g h i j k l m n o p q r s t u v w x y z)
    printf "$base_url/general_ref_%s.pdf\0" "${cmd_refs[@]}" | xargs --null --max-args=1 --max-procs=26 -- \
        $curl "${curl_opts[@]}" --output-dir "$1"

    # Misc docs
    declare -a -r cmd_misc=(commandlist)
    printf "$base_url/%s.pdf\0" "${cmd_misc[@]}" | xargs --null --max-args=1 --max-procs=0 -- \
        $curl "${curl_opts[@]}" --output-dir "$1"
}

function convert () {
    readonly pdftotext="/usr/bin/pdftotext"

    declare -a -r pdftotext_opts_man=("-layout" "-x" "0" "-y" "0" "-W" "4000" "-H" "675")

    find "$1" -type f -name "*.pdf" -print0 | xargs --null --max-procs=0 --replace=IN -- \
        $pdftotext "${pdftotext_opts_man[@]}" IN IN.man.txt

    declare -a -r pdftotext_opts_copyright=("-layout" "-x" "0" "-y" "675" "-W" "4000" "-H" "1000")

    find "$1" -type f -name "*.pdf" -print0 | xargs --null --max-procs=0 --replace=IN -- \
        $pdftotext "${pdftotext_opts_copyright[@]}" IN IN.copyright.txt
}

trap 'on_error $LINENO' ERR

do_functions=false
do_commands=false

while getopts "cf" arg; do
    case $arg in
        f) do_functions=true ;;
        c) do_commands=true ;;
        *) echo "ERROR: Invalid option \"-$OPTARG\"" >&2; usage ;;
    esac
done

if [ "$do_functions" = true ]; then
    shift $((OPTIND - 1))
fi

if [ $# -eq 0 ]; then
    echo "ERROR: No input directory specified" >&2
    exit 1
elif [ $# -gt 1 ]; then
    echo "ERROR: Too many positional arguments specified" >&2
    exit 1
fi

if [[ ! -d "$1" ]]; then
    echo "ERROR: Directory \"$1\" does not exist. Exiting..."
    exit 1
fi

if [ ! "$do_functions" = true ] && [ ! "$do_commands" = true ]; then
    echo "ERROR: Both \"-f\" and \"-c\" are missing" >&2
    exit 1
fi

if [ "$do_functions" = true ]; then
    download_functions "$1"
fi

if [ "$do_commands" = true ]; then
    download_commands "$1"
fi

convert "$1"
