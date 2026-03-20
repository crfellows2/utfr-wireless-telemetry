# BLE Telemetry Scheduling Algorithm

## Overview

Given a set of signals with defined priorities and frequency bounds, and a current
maximum link throughput, compute a target frequency for each signal that maximizes
data throughput while respecting priority ordering.

## Signal Definition

Each signal has:
- `can_id` — CAN frame identifier
- `min_frequency_hz` — minimum acceptable frequency, below which the signal is useless
- `frequency_hz` — desired maximum frequency
- `priority` — integer, lower number = higher priority

Signals are grouped into **buckets** by priority level. All signals within a bucket
are treated equally.

## Scheduling Algorithm

Each bucket has an **interpolation scalar** `s ∈ [0.0, 1.0]` which scales all signals
in that bucket between their minimum and desired frequencies:

```
current_hz = min_frequency_hz + s * (frequency_hz - min_frequency_hz)
```

At `s = 0.0` all signals run at `min_frequency_hz`.  
At `s = 1.0` all signals run at `frequency_hz`.

**Algorithm (fill from highest priority down):**

```
budget = max_throughput
set all bucket scalars to 0.0

for each bucket (highest priority first):

    // How much bandwidth does this bucket cost at full scalar?
    cost_at_max = sum(frequency_hz for each signal in bucket)
    cost_at_min = sum(min_frequency_hz for each signal in bucket)
    available = cost_at_max - cost_at_min

    if budget >= cost_at_max:
        // Full budget available, run at maximum
        scalar = 1.0
        budget -= cost_at_max

    else if budget >= cost_at_min:
        // Partial budget, interpolate scalar to exactly exhaust remaining budget
        scalar = (budget - cost_at_min) / available
        budget = 0
        break  // no budget left for lower priority buckets

    else:
        // Not enough budget even for minimums, drop entire bucket
        // (leave scalar at 0.0 and don't subtract from budget)
        break  // no point continuing to lower priority buckets
```

Any bucket whose scalar remains 0.0 after the algorithm is dropped entirely —
its signals are not transmitted.

## Sampling

The scheduler outputs a target frequency per signal:

```
can_id → target_hz
```

A **token bucket** per signal is used to sample incoming CAN frames at the target rate:
- Each signal accumulates tokens at `target_hz`
- When a CAN frame arrives, consume one token and forward the frame
- If no token is available, drop the frame

This naturally handles the mismatch between CAN frame arrival cadence and target
frequencies without requiring per-signal timers.

## Notes

- Re-run the scheduling algorithm whenever `max_throughput` changes significantly
- The scalar per bucket is a useful live diagnostic of link utilization
- Signals with `min_frequency_hz == frequency_hz` behave as fixed-rate — they are
  either fully included or dropped, no interpolation
- Signals with `min_frequency_hz == 0` can be reduced all the way to zero before
  being dropped, maximizing graceful degradation
