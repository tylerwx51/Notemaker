# Sparse selection reference validation — NoteAir6C, 2026-10-21

Selection taps first use exact DrawItem coverage in the Android editor to choose an ID. The local proxy then validates those explicit IDs and grants a transient lease. Previously its offline path built a full LayerEditor three times: once before acquisition, again in the shared selection method, and again inside the atomic coordination update. The proxy now validates each requested ID directly against the layer, page, and active flags, skips the redundant outer pass, and retains the final check inside the atomic update. The durable model and selection protocol are unchanged.

Two runs on the physical NoteAir6C used the same copied note and isolated interaction suite as the [selection baseline](selection-tap-baseline.md). Each run had 35 measured selection taps after 5 warmups. Values are median / p95 milliseconds.

| Metric | Baseline run 1 / 2 | Candidate run 1 / 2 |
| --- | ---: | ---: |
| Input to first updated UI draw | 84.2 / 90.5; 80.1 / 89.8 | 37.0 / 61.6; 37.5 / 62.3 |
| Input to updated Canvas draw | 93.9 / 108.5; 91.6 / 103.4 | 49.3 / 78.4; 50.1 / 78.5 |
| Tap handler | 16.8 / 21.1; 16.6 / 21.1 | 19.1 / 23.1; 20.7 / 23.5 |
| Handler to Canvas draw | 76.5 / 93.8; 72.1 / 87.9 | 33.5 / 56.1; 30.0 / 55.0 |

The final candidate adds an atomic check that an existing session has not moved to another layer or page. One more physical run on that code measured 39.2 / 63.1 ms to the first updated UI draw and 52.2 / 81.0 ms to Canvas draw, with 5.07 seconds of process CPU and zero charged disk writes for 40 acquire/release cycles. The same local CLI/proxy suite passed after this check, including a new same-session cross-scope rejection test.

The 40 acquire/release cycles used 6.20–6.21 seconds of process CPU in the baseline and 5.00–5.11 seconds with sparse validation. The sampled PSS increase during this mode fell from about 33 MiB to about 18 MiB; these before/after samples include collection timing and do not prove a retained-memory reduction. Both versions charged zero disk writes. Other action latencies in the suite stayed near baseline. The local CLI/proxy selection tests passed, including new checks that reject wrong-layer, wrong-page, wrong-kind, missing, and hidden-layer references without leaving a lease. The full Android emulator drawing suite passed its selection, lease, preview, pixel, clipboard, and panel checks.

Raw traces, resource counters, and APK hashes are under `.test-artifacts/hillclimb/tablet-selection-sparse-{1,2}/` and `tablet-selection-sparse-final-1/`. Timings end at Android drawing and exclude physical touch sampling and E Ink scanout.
