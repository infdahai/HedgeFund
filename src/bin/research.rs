#[cfg(not(feature = "polars"))]
fn main() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "research binary requires --features polars"
    ))
}

#[cfg(feature = "polars")]
fn main() -> anyhow::Result<()> {
    opinion_hft::research::run()
}
