#!/bin/sh
case "$GIT_COMMIT" in
5fb564bf32920eb133c072b410d996e61da9759b)
  printf "%s\n" "Fix incorrect amount of time recorded for failover, and use SSH connection warming to speed failover. "
  ;;
f7da91cdb6e2dc648b2941a3e5853e83901c5595)
  printf "%s\n" "Fixed tower checks on verification race that caused failover failures on Fire Dancer"
  ;;
2b4cc39ba396bda0190d758dfa50fdd77ea599b6)
  printf "%s\n" "Fix startup identity check that caused fire dancer detection to fail. "
  ;;
*)
  cat
  ;;
esac
