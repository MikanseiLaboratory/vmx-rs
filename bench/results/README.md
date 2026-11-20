# Result CSVs

`gcp-gpu-l4-images.csv` is the L4 image pass (`e1c68a2`, `run_gpu.sh`). `ms_median` is already rounded to 2 decimal places and `fps_median` to an integer. `ms_samples` is empty. Dividing 1000 by `ms_median` does not always reproduce `fps_median`, because both columns were rounded before they were written down.

The other CSVs keep the precision recorded by the harness that produced them.
