#!/bin/bash
# Resume script from the 2026-11-20 GCP run. It assumes a checkout at
# ~/vmx-rs and appends OMT-only rows onto an existing results file.
# The published tables were built from the finished CSVs in bench/results/.
. ~/.cargo/env; cd ~/vmx-rs/bench; OUT=~/out
for combo in "1080 bars" "2160 natural" "2160 bars"; do set -- $combo
  for p in OMT_LQ OMT_SQ OMT_HQ; do
    cargo run --release --locked -q -- --res $1 --img $2 --prof $p 2>>$OUT/run_omt.err | tail -n +2 >> $OUT/results_omt_resume.csv
  done; done
echo DONE > $OUT/DONE2
