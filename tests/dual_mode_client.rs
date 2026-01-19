use opinion_hft::config::{Config, Environment};
use opinion_hft::data::rest::OpinionClient;
use std::str::FromStr;

#[test]
fn test_environment_parsing() {
    assert_eq!(Environment::from_str("local"), Ok(Environment::Local));
    assert_eq!(Environment::from_str("LOCAL"), Ok(Environment::Local));
    assert_eq!(Environment::from_str("aws"), Ok(Environment::Aws));
    assert_eq!(Environment::from_str("AWS"), Ok(Environment::Aws));
    assert!(Environment::from_str("invalid").is_err());
}

#[test]
fn test_environment_defaults() {
    assert_eq!(Environment::default(), Environment::Local);
    assert!(!Environment::Aws.is_local());
    assert!(Environment::Local.is_local());
}

#[tokio::test]

async fn test_local_mode_uses_proxy() {
    dotenvy::dotenv().ok();

    let config = Config::builder()
        .environment(Environment::Local)
        .api_key("test-key")
        .rpc_url("https://bsc-dataseed.binance.org")
        .private_key("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
        .build_unchecked();

    let _client = OpinionClient::new(&config.api, config.environment).unwrap();
    assert!(config.environment.is_local());
}

#[tokio::test]

async fn test_aws_mode_uses_direct_connection() {
    dotenvy::dotenv().ok();

    let config = Config::builder()
        .environment(Environment::Aws)
        .api_key("test-key")
        .rpc_url("https://bsc-dataseed.binance.org")
        .private_key("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
        .build_unchecked();

    let _client = OpinionClient::new(&config.api, config.environment).unwrap();
    assert!(!config.environment.is_local());
}

#[tokio::test]

async fn test_env_var_loading() {
    std::env::set_var("OPINION_ENV", "local");
    std::env::set_var("OPINION_API_KEY", "test-key");
    std::env::set_var(
        "OPINION_EOA_ADDRESS",
        "0x0000000000000000000000000000000000000000",
    );
    std::env::set_var(
        "OPINION_PRIVATE_KEY",
        "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
    );
    std::env::set_var("BSC_RPC_URL", "https://bsc-dataseed.binance.org");

    let config = Config::from_env().unwrap();
    assert_eq!(config.environment, Environment::Local);
    assert!(config.environment.is_local());

    std::env::set_var("OPINION_ENV", "aws");
    let config = Config::from_env().unwrap();
    assert_eq!(config.environment, Environment::Aws);
    assert!(!config.environment.is_local());
}
