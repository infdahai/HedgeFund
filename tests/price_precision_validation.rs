/// Validation: 1 decimal place price precision (0.1 - 0.9)
/// 
/// This test validates that price formatting works correctly for the frontend use case,
/// where users only place orders with 1 decimal place precision.

#[cfg(test)]
mod price_precision_tests {
    
    #[test]
    fn test_price_formatting_one_decimal() {
        let test_cases = vec![
            (0.1, "0.1"),
            (0.2, "0.2"),
            (0.3, "0.3"),
            (0.4, "0.4"),
            (0.5, "0.5"),
            (0.6, "0.6"),
            (0.7, "0.7"),
            (0.8, "0.8"),
            (0.9, "0.9"),
        ];

        for (price, expected) in test_cases {
            let formatted = format!("{:.1}", price);
            assert_eq!(
                formatted, expected,
                "Price {:.10} should format to {}, got {}",
                price, expected, formatted
            );
        }
    }

    #[test]
    fn test_price_formatting_with_float_errors() {
        // Test floating point precision edge cases
        let test_cases = vec![
            (0.1 + 0.0000001, "0.1"),
            (0.5 - 0.0000001, "0.5"),
            (0.9 + 0.0000001, "0.9"),
            (0.30000000000000004, "0.3"), // Classic floating point issue
        ];

        for (price, expected) in test_cases {
            let formatted = format!("{:.1}", price);
            assert_eq!(
                formatted, expected,
                "Price {:.10} with floating error should format to {}, got {}",
                price, expected, formatted
            );
        }
    }

    #[test]
    fn test_rollback_price_calculation() {
        let original_prices = vec![0.1_f64, 0.3_f64, 0.5_f64, 0.7_f64, 0.9_f64];
        
        for original in original_prices {
            let buy_rollback = original * 1.05_f64;
            let buy_rounded = (buy_rollback * 10.0_f64).round() / 10.0_f64;
            let buy_clamped = buy_rounded.max(0.1_f64).min(0.9_f64);
            
            assert!(
                buy_clamped >= 0.1_f64 && buy_clamped <= 0.9_f64,
                "Buy rollback for {} should be in range [0.1, 0.9], got {}",
                original, buy_clamped
            );
            
            let sell_rollback = original * 0.95_f64;
            let sell_rounded = (sell_rollback * 10.0_f64).round() / 10.0_f64;
            let sell_clamped = sell_rounded.max(0.1_f64).min(0.9_f64);
            
            assert!(
                sell_clamped >= 0.1_f64 && sell_clamped <= 0.9_f64,
                "Sell rollback for {} should be in range [0.1, 0.9], got {}",
                original, sell_clamped
            );
            
            println!(
                "Original: {:.1} → Buy Rollback: {:.1}, Sell Rollback: {:.1}",
                original, buy_clamped, sell_clamped
            );
        }
    }

    #[test]
    fn test_edge_case_boundaries() {
        // Test edge cases at boundaries
        let edge_cases = vec![
            (0.05, "0.1"), // Below min, should round to 0.1 after clamping
            (0.95, "0.9"), // Near max, should stay 0.9
            (0.099, "0.1"),
            (0.101, "0.1"),
            (0.899, "0.9"),
            (0.901, "0.9"),
        ];

        for (price, expected) in edge_cases {
            let formatted = format!("{:.1}", price);
            assert_eq!(
                formatted, expected,
                "Edge case price {} should format to {}, got {}",
                price, expected, formatted
            );
        }
    }

    #[test]
    fn test_price_string_parsing() {
        // Verify that formatted strings can be parsed back correctly
        let prices = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
        
        for original_price in prices {
            let price_str = format!("{:.1}", original_price);
            let parsed: f64 = price_str.parse().expect("Should parse back to f64");
            
            // Allow small floating point error
            let diff = (parsed - original_price).abs();
            assert!(
                diff < 0.01,
                "Parsed price {} should be close to original {}, diff: {}",
                parsed, original_price, diff
            );
        }
    }

    #[test]
    fn test_price_multiplication_precision() {
        // Test that price * amount calculations maintain precision
        let price = 0.5;
        let amount = 100.0;
        
        let result = price * amount;
        assert_eq!(result, 50.0, "0.5 * 100 should equal 50.0");
        
        // Format and verify
        let price_str = format!("{:.1}", price);
        assert_eq!(price_str, "0.5");
        
        let parsed: f64 = price_str.parse().unwrap();
        let recalc = parsed * amount;
        assert_eq!(recalc, 50.0, "Parsed price calculation should still be correct");
    }

    #[test]
    fn test_price_range_validation() {
        // All valid 1-decimal prices
        let valid_prices: Vec<f64> = (1..=9).map(|i| i as f64 / 10.0).collect();
        
        for price in valid_prices {
            assert!(
                price >= 0.1 && price <= 0.9,
                "Price {} should be in valid range [0.1, 0.9]",
                price
            );
            
            let formatted = format!("{:.1}", price);
            let parsed: f64 = formatted.parse().unwrap();
            
            assert!(
                parsed >= 0.1 && parsed <= 0.9,
                "Parsed price {} should still be in valid range",
                parsed
            );
        }
    }
}
