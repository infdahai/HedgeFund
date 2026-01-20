# Verification

## Parquet Validation (Python)

Use the project venv to avoid system Python conflicts. This reads a single file,
prints schema + sample rows, and validates nested bids.

```bash
".venv/bin/python" - <<'PY'
import glob
import polars as pl

files = glob.glob("data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet")
if not files:
    raise SystemExit("No parquet files found under data/parquet/orderbook_snapshots_v1")

target = files[0]
print(f"Checking file: {target}")

df = pl.read_parquet(target, hive_partitioning=True)
print("\n--- Schema ---")
print(df.schema)

print("\n--- Data Preview ---")
print(df.select(["local_recv_ts_us", "market_id", "bids", "date", "hour"]).head(5))

bids_list = df["bids"].to_list()
first_bids = bids_list[0] if bids_list else []
print(f"\nFirst row bids count: {len(first_bids or [])}")
if first_bids:
    print(f"Top bid: {first_bids[0]}")
PY
```

## Parquet Validation (DuckDB)

See `docs/verify_parquet.sql` for the DuckDB version.
