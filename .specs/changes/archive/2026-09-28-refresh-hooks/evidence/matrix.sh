#!/bin/sh
# Runs a chaining simulation (argument 1: script path) over every install/uninstall order tested.
script="$1"
for seq in \
  "prek c10r prek c10r-un prek-un" \
  "H prek c10r prek c10r-un prek-un" \
  "H prek c10r prek prek-un c10r-un" \
  "H c10r prek c10r-un prek-un" \
  "H c10r prek prek-un c10r-un" \
  "H prek c10r c10r-un prek-un" \
  "c10r prek c10r-un prek-un" \
  "c10r prek prek-un c10r-un" \
  "H prek c10r prek-f c10r c10r-un prek-un" \
  "prek c10r prek prek-f c10r c10r-un prek-un" \
  "H c10r prek prek-f c10r c10r-un prek-un"; do
  echo "=== ${seq}"
  sh "${script}" "${seq}"
done
