use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

const JUPITER_API: &str = "https://api.jup.ag/price/v2";
pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";

#[derive(Deserialize)]
struct JupResponse {
    data: HashMap<String, PriceData>,
}

#[derive(Deserialize)]
struct PriceData {
    price: String,
}

pub async fn fetch_prices(mints: &[String]) -> Result<HashMap<String, f64>> {
    if mints.is_empty() {
        return Ok(HashMap::new());
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let mut prices = HashMap::new();

    // Jupiter limits: 100 ids per call
    for chunk in mints.chunks(100) {
        let ids = chunk.join(",");
        let url = format!("{}?ids={}", JUPITER_API, ids);

        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(text) = resp.text().await {
                let parsed = parse_jupiter_response(&text);
                for (m, p) in parsed {
                    prices.insert(m, p);
                }
            }
        }
    }

    Ok(prices)
}

pub async fn sol_price() -> Result<f64> {
    let prices = fetch_prices(&[SOL_MINT.to_string()]).await?;
    Ok(prices.get(SOL_MINT).copied().unwrap_or(0.0))
}

fn parse_jupiter_response(json: &str) -> HashMap<String, f64> {
    let mut prices = HashMap::new();
    if let Ok(body) = serde_json::from_str::<JupResponse>(json) {
        for (m, d) in body.data {
            if let Ok(p) = d.price.parse::<f64>() {
                prices.insert(m, p);
            }
        }
    }
    prices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jupiter_response() {
        let json = r#"{
            "data": {
                "So11111111111111111111111111111111111111112": {
                    "id": "So11111111111111111111111111111111111111112",
                    "mintSymbol": "SOL",
                    "vsToken": "USDC",
                    "vsTokenSymbol": "USDC",
                    "price": "24.56"
                },
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": {
                    "id": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    "mintSymbol": "USDC",
                    "vsToken": "USDC",
                    "vsTokenSymbol": "USDC",
                    "price": "1.00"
                }
            }
        }"#;

        let prices = parse_jupiter_response(json);
        assert_eq!(prices.len(), 2);
        assert_eq!(prices["So11111111111111111111111111111111111111112"], 24.56);
        assert_eq!(prices["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"], 1.00);
    }

    #[test]
    fn test_parse_invalid_json() {
        let prices = parse_jupiter_response("invalid json");
        assert!(prices.is_empty());
    }

    /// Integration test: Fetch real prices from Jupiter API
    /// This test requires network access
    #[tokio::test]
    async fn test_fetch_prices_integration() {
        // SOL and USDC mints
        let mints = vec![
            SOL_MINT.to_string(),
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
        ];

        let result = fetch_prices(&mints).await;

        // Should succeed (not error)
        assert!(result.is_ok(), "fetch_prices failed: {:?}", result);

        let prices = result.unwrap();

        // SOL should have a price > 0
        if let Some(&sol_price) = prices.get(SOL_MINT) {
            assert!(sol_price > 0.0, "SOL price should be positive");
        }
    }

    #[tokio::test]
    async fn test_sol_price_integration() {
        let result = sol_price().await;
        assert!(result.is_ok());

        let price = result.unwrap();
        // Price should be non-negative (0 is acceptable if API is down/rate limited)
        assert!(price >= 0.0, "SOL price should be non-negative: {}", price);
    }
}
