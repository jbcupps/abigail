//! Web search capability via Tavily API.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct TavilySearchRequest {
    pub api_key: String,
    pub query: String,
    pub search_depth: String,
    pub include_answer: bool,
    pub max_results: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TavilySearchResponse {
    pub answer: Option<String>,
    pub results: Vec<TavilyResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TavilyResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: Option<f64>,
}

/// Search the web using the Tavily API.
pub async fn tavily_search(
    api_key: &str,
    query: &str,
    max_results: u32,
) -> Result<TavilySearchResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let request = TavilySearchRequest {
        api_key: api_key.to_string(),
        query: query.to_string(),
        search_depth: "basic".to_string(),
        include_answer: true,
        max_results,
    };

    let resp = client
        .post("https://api.tavily.com/search")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Tavily request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Tavily API error ({}): {}", status, body));
    }

    resp.json::<TavilySearchResponse>()
        .await
        .map_err(|e| format!("Failed to parse Tavily response: {}", e))
}

/// Format search results into a human-readable string for LLM consumption.
pub fn format_search_results(response: &TavilySearchResponse) -> String {
    let mut out = String::new();

    if let Some(ref answer) = response.answer {
        out.push_str(answer);
        out.push_str("\n\n");
    }

    if !response.results.is_empty() {
        out.push_str("Sources:\n");
        for (i, r) in response.results.iter().enumerate() {
            out.push_str(&format!("{}. {} — {}\n   {}\n", i + 1, r.title, r.url, r.content));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_search_results_with_answer() {
        let response = TavilySearchResponse {
            answer: Some("NVIDIA stock is at $120.".to_string()),
            results: vec![
                TavilyResult {
                    title: "NVIDIA Stock Price".to_string(),
                    url: "https://example.com/nvidia".to_string(),
                    content: "NVIDIA (NVDA) is trading at $120.".to_string(),
                    score: Some(0.95),
                },
            ],
        };

        let formatted = format_search_results(&response);
        assert!(formatted.contains("NVIDIA stock is at $120."));
        assert!(formatted.contains("Sources:"));
        assert!(formatted.contains("1. NVIDIA Stock Price"));
        assert!(formatted.contains("https://example.com/nvidia"));
    }

    #[test]
    fn test_format_search_results_empty() {
        let response = TavilySearchResponse {
            answer: None,
            results: vec![],
        };
        let formatted = format_search_results(&response);
        assert!(formatted.is_empty());
    }
}
