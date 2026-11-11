//! BASE (Bielefeld Academic Search Engine) search engine implementation
//!
//! Search for academic web resources and scholarly publications.
//! Reference: python-engines/base.py

use super::traits::*;
use crate::results::{Result, ResultType};
use anyhow::Result as AnyhowResult;
use std::collections::HashMap;

/// BASE academic search engine
pub struct Base {
    base_url: String,
}

impl Base {
    pub fn new() -> Self {
        Self {
            base_url: "https://api.base-search.net/cgi-bin/BaseHttpSearchInterface.fcgi"
                .to_string(),
        }
    }

    /// Parse BASE XML response
    fn parse_results(&self, xml_content: &str) -> Vec<Result> {
        let mut results = Vec::new();
        let mut position = 1u32;

        // Split into individual entries
        for entry in xml_content.split("<doc>") {
            if entry.is_empty() {
                continue;
            }

            let mut title = String::new();
            let mut url = String::new();
            let mut content = String::new();
            let mut published_date: Option<String> = None;

            // Extract dctitle
            if let Some(title_start) = entry.find("<dctitle>") {
                let content_start = title_start + 9; // length of <dctitle>
                if let Some(title_end) = entry[content_start..].find("</dctitle>") {
                    title = entry[content_start..content_start + title_end]
                        .trim()
                        .to_string();
                }
            }

            // Extract dclink
            if let Some(link_start) = entry.find("<dclink>") {
                let content_start = link_start + 8; // length of <dclink>
                if let Some(link_end) = entry[content_start..].find("</dclink>") {
                    url = entry[content_start..content_start + link_end]
                        .trim()
                        .to_string();
                }
            }

            // Extract dcdescription
            if let Some(desc_start) = entry.find("<dcdescription>") {
                let content_start = desc_start + 15; // length of <dcdescription>
                if let Some(desc_end) = entry[content_start..].find("</dcdescription>") {
                    content = entry[content_start..content_start + desc_end]
                        .trim()
                        .to_string();
                    if content.len() > 300 {
                        let end = content.floor_char_boundary(300);
                        content = format!("{}...", &content[..end]);
                    }
                }
            }

            // Extract dcdate
            if let Some(date_start) = entry.find("<dcdate>") {
                let content_start = date_start + 8; // length of <dcdate>
                if let Some(date_end) = entry[content_start..].find("</dcdate>") {
                    let date_str = &entry[content_start..content_start + date_end].trim();
                    // Parse various date formats
                    let date_formats = ["%Y-%m-%dT%H:%M:%SZ", "%Y-%m-%d", "%Y-%m", "%Y"];
                    for format in date_formats.iter() {
                        if let Ok(dt) = chrono::DateTime::parse_from_str(date_str, format) {
                            published_date = Some(dt.format("%Y-%m-%d").to_string());
                            break;
                        }
                    }
                }
            }

            // Skip if no title
            if title.is_empty() {
                continue;
            }

            // Skip if no URL
            if url.is_empty() {
                continue;
            }

            let mut result = Result::new(url.clone(), title, self.name().to_string());
            result.result_type = ResultType::Paper;
            result = result.with_position(position);

            if !content.is_empty() {
                result = result.with_content(content);
            }

            if let Some(date) = published_date {
                result.metadata.published_date = Some(date);
            }

            results.push(result);
            position += 1;
        }

        results
    }
}

impl Default for Base {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for Base {
    fn name(&self) -> &str {
        "base"
    }

    fn about(&self) -> EngineAbout {
        EngineAbout::new()
            .website("https://www.base-search.net")
            .official_api(true)
            .results_format("XML")
    }

    fn categories(&self) -> Vec<&str> {
        vec!["science", "academic", "research"]
    }

    fn supports_paging(&self) -> bool {
        true
    }

    fn request(&self, params: &RequestParams) -> AnyhowResult<EngineRequest> {
        let offset = (params.pageno - 1) * 10;

        // Build query with advanced search shortcuts support
        let mut query = params.query.clone();

        // Replace common shortcuts with BASE API keywords
        let shortcuts = [
            ("format:", "dcformat:"),
            ("author:", "dccreator:"),
            ("collection:", "dccollection:"),
            ("hdate:", "dchdate:"),
            ("contributor:", "dccontributor:"),
            ("coverage:", "dccoverage:"),
            ("date:", "dcdate:"),
            ("abstract:", "dcdescription:"),
            ("urls:", "dcidentifier:"),
            ("language:", "dclanguage:"),
            ("publisher:", "dcpublisher:"),
            ("relation:", "dcrelation:"),
            ("rights:", "dcrights:"),
            ("source:", "dcsource:"),
            ("subject:", "dcsubject:"),
            ("title:", "dctitle:"),
            ("type:", "dcdctype:"),
        ];

        for (shortcut, api_field) in shortcuts.iter() {
            query = query.replace(shortcut, api_field);
        }

        let mut query_params = HashMap::new();
        query_params.insert("func".to_string(), "PerformSearch".to_string());
        query_params.insert(
            "query".to_string(),
            format!("query={}", urlencoding::encode(&query)),
        );
        query_params.insert("hits".to_string(), "10".to_string());
        query_params.insert("offset".to_string(), offset.to_string());
        query_params.insert("boost".to_string(), "oa".to_string());

        let mut request = EngineRequest::get(&self.base_url);
        request.params = query_params;

        Ok(request)
    }

    fn response(&self, response: EngineResponse) -> AnyhowResult<EngineResults> {
        if !response.is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status));
        }

        let results = self.parse_results(&response.text);

        Ok(EngineResults::with_results(results))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_request() {
        let base = Base::new();
        let params = RequestParams::new("quantum computing");
        let request = base.request(&params).unwrap();

        assert!(request.url.contains("base-search.net"));
        assert!(request.params.contains_key("query"));
        assert_eq!(
            request.params.get("func"),
            Some(&"PerformSearch".to_string())
        );
    }

    #[test]
    fn test_base_categories() {
        let base = Base::new();
        let categories = base.categories();

        assert!(categories.contains(&"science"));
        assert!(categories.contains(&"academic"));
        assert!(categories.contains(&"research"));
    }

    #[test]
    fn test_base_paging() {
        let base = Base::new();
        assert!(base.supports_paging());
    }

    #[test]
    fn test_base_shortcuts() {
        let base = Base::new();
        let params = RequestParams::new("author:Smith");
        let request = base.request(&params).unwrap();

        assert!(request.params.contains_key("query"));
        let query_param = request.params.get("query").unwrap();
        // Query is URL-encoded, so check for encoded version
        assert!(query_param.contains("dccreator%3ASmith") || query_param.contains("dccreator:"));
    }
}
