SELECT
    market_id,
    local_recv_ts_us,
    bids[1].p AS best_bid_price,
    bids[1].s AS best_bid_size,
    date,
    hour
FROM read_parquet('data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet', hive_partitioning=true)
ORDER BY local_recv_ts_us DESC
LIMIT 10;
