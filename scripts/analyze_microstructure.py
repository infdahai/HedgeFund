import polars as pl
import argparse
import sys


def analyze_obi_predictability(features_path: str):
    """Analyze OBI predictability from extracted features."""
    try:
        df = pl.read_parquet(features_path)
    except Exception as e:
        print(f"Error reading {features_path}: {e}")
        return

    print(f"Loaded {len(df)} rows from {features_path}")

    # Calculate return (10s)
    # return = (next_price - mid_price) / mid_price
    # (already in price_change_10s but let's be explicit if needed, or use the pre-calc one)

    if "price_change_10s" not in df.columns:
        print("price_change_10s column missing.")
        return

    df = df.with_columns([pl.col("price_change_10s").alias("return_10s")])

    # Define OBI buckets
    # Strong Buy: > 0.7
    # Strong Sell: < -0.7

    df = df.with_columns(
        [
            pl.when(pl.col("obi_l1") > 0.7)
            .then(pl.lit("Strong Buy (>0.7)"))
            .when(pl.col("obi_l1") > 0.3)
            .then(pl.lit("Weak Buy (>0.3)"))
            .when(pl.col("obi_l1") < -0.7)
            .then(pl.lit("Strong Sell (<-0.7)"))
            .when(pl.col("obi_l1") < -0.3)
            .then(pl.lit("Weak Sell (<-0.3)"))
            .otherwise(pl.lit("Neutral"))
            .alias("signal")
        ]
    )

    # Filter null returns (where no next price was found)
    df_valid = df.filter(pl.col("return_10s").is_not_null())

    print("\n=== OBI Signal Analysis (10s Horizon) ===")
    stats = (
        df_valid.group_by("signal")
        .agg(
            [
                pl.count().alias("count"),
                (pl.col("return_10s") > 0).mean().alias("win_rate"),
                (pl.col("return_10s") < 0).mean().alias("loss_rate"),
                pl.col("return_10s").mean().alias("avg_return"),
                pl.col("return_10s").std().alias("std_return"),
                # Median spread to ensure we can actually execute
                pl.col("spread_bps").median().alias("median_spread_bps"),
            ]
        )
        .sort("signal")
    )

    print(stats)

    # Specific check for Strong Buy/Sell
    strong_buy = stats.filter(pl.col("signal") == "Strong Buy (>0.7)")
    if len(strong_buy) > 0:
        win_rate = strong_buy["win_rate"][0]
        avg_ret = strong_buy["avg_return"][0]
        print(f"\nStrong Buy Win Rate: {win_rate:.2%}")
        print(f"Strong Buy Avg Return: {avg_ret:.4%}")

        if win_rate > 0.55:
            print("\n✅ GREEN LIGHT: Win rate > 55%")
        else:
            print("\n❌ RED LIGHT: Win rate <= 55%")
    else:
        print("\nNo Strong Buy signals found.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("input", help="Path to features.parquet")
    args = parser.parse_args()

    analyze_obi_predictability(args.input)
