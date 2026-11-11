//! Wikipedia search engine implementation

use super::traits::*;
use crate::results::{InfoBox, Result};
use anyhow::Result as AnyhowResult;
use std::collections::HashMap;

/// Wikipedia search engine
pub struct Wikipedia {
    api_url: String,
    default_lang: String,
}

impl Wikipedia {
    pub fn new() -> Self {
        Self {
            api_url: "https://{lang}.wikipedia.org/w/api.php".to_string(),
            default_lang: "en".to_string(),
        }
    }

    fn get_api_url(&self, lang: &str) -> String {
        let lang = if lang == "all" || lang.is_empty() {
            &self.default_lang
        } else {
            // Extract base language code (e.g., "en" from "en-US")
            lang.split('-').next().unwrap_or(&self.default_lang)
        };
        self.api_url.replace("{lang}", lang)
    }
}

impl Default for Wikipedia {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for Wikipedia {
    fn name(&self) -> &str {
        "wikipedia"
    }

    fn about(&self) -> EngineAbout {
        EngineAbout::new()
            .website("https://www.wikipedia.org")
            .official_api(true)
            .results_format("JSON")
    }

    fn categories(&self) -> Vec<&str> {
        vec!["general"]
    }

    fn supports_paging(&self) -> bool {
        true
    }

    fn request(&self, params: &RequestParams) -> AnyhowResult<EngineRequest> {
        let api_url = self.get_api_url(&params.lang);

        let mut query_params = HashMap::new();
        query_params.insert("action".to_string(), "query".to_string());
        query_params.insert("format".to_string(), "json".to_string());
        query_params.insert("generator".to_string(), "search".to_string());
        query_params.insert("gsrsearch".to_string(), params.query.clone());
        query_params.insert("gsrlimit".to_string(), "10".to_string());
        query_params.insert("prop".to_string(), "extracts|pageimages|info".to_string());
        query_params.insert("exintro".to_string(), "1".to_string());
        query_params.insert("explaintext".to_string(), "1".to_string());
        query_params.insert("exlimit".to_string(), "10".to_string());
        query_params.insert("inprop".to_string(), "url".to_string());
        query_params.insert("pithumbsize".to_string(), "300".to_string());

        // Pagination
        if params.pageno > 1 {
            let offset = (params.pageno - 1) * 10;
            query_params.insert("gsroffset".to_string(), offset.to_string());
        }

        let mut request = EngineRequest::get(&api_url);
        request.params = query_params;

        Ok(request)
    }

    fn response(&self, response: EngineResponse) -> AnyhowResult<EngineResults> {
        if !response.is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status));
        }

        let json: serde_json::Value = serde_json::from_str(&response.text)?;
        let mut engine_results = EngineResults::new();

        // Parse pages from response
        if let Some(pages) = json
            .get("query")
            .and_then(|q| q.get("pages"))
            .and_then(|p| p.as_object())
        {
            let mut position = 1u32;

            // Sort by index to maintain search relevance order
            let mut page_list: Vec<_> = pages.values().collect();
            page_list.sort_by(|a, b| {
                let idx_a = a.get("index").and_then(|i| i.as_i64()).unwrap_or(999);
                let idx_b = b.get("index").and_then(|i| i.as_i64()).unwrap_or(999);
                idx_a.cmp(&idx_b)
            });

            for page in page_list {
                let title = page
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default();

                let url = page
                    .get("fullurl")
                    .and_then(|u| u.as_str())
                    .unwrap_or_default();

                if title.is_empty() || url.is_empty() {
                    continue;
                }

                let extract = page
                    .get("extract")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string());

                let thumbnail = page
                    .get("thumbnail")
                    .and_then(|t| t.get("source"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());

                let mut result =
                    Result::new(url.to_string(), title.to_string(), self.name().to_string());

                if let Some(content) = extract {
                    // Truncate long extracts (char-boundary safe)
                    let truncated = if content.len() > 500 {
                        let end = content.floor_char_boundary(500);
                        format!("{}...", &content[..end])
                    } else {
                        content
                    };
                    result = result.with_content(truncated);
                }

                if let Some(thumb) = thumbnail {
                    result.metadata.thumbnail = Some(thumb);
                }

                result = result.with_position(position);
                position += 1;

                engine_results.add_result(result);
            }
        }

        Ok(engine_results)
    }
}

/// Wikipedia Infobox fetcher (for detailed article info)
pub struct WikipediaInfobox {
    _api_url: String,
}

impl WikipediaInfobox {
    pub fn new() -> Self {
        Self {
            _api_url: "https://en.wikipedia.org/api/rest_v1/page/summary/".to_string(),
        }
    }

    /// Fetch infobox for a specific article
    pub async fn fetch(&self, _title: &str) -> AnyhowResult<Option<InfoBox>> {
        // This would be called separately for detailed article info
        // For now, return None - would need HTTP client access
        Ok(None)
    }
}

impl Default for WikipediaInfobox {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wikipedia_request() {
        let wiki = Wikipedia::new();
        let params = RequestParams::new("rust programming");
        let request = wiki.request(&params).unwrap();

        assert!(request.url.contains("wikipedia.org"));
        assert!(request.params.contains_key("gsrsearch"));
    }

    #[test]
    fn test_language_url() {
        let wiki = Wikipedia::new();
        assert!(wiki.get_api_url("de").contains("de.wikipedia.org"));
        assert!(wiki.get_api_url("en-US").contains("en.wikipedia.org"));
        assert!(wiki.get_api_url("all").contains("en.wikipedia.org"));
    }
}
