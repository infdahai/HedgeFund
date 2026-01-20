#!/usr/bin/env bash
set -euo pipefail

base_dir="${1:-/opt/opinion-hft}"

install -d "${base_dir}/data" "${base_dir}/logs"
