//! arXiv search engine implementation
//!
//! Uses the arXiv API to search for scholarly articles in physics, mathematics,
//! computer science, and other fields.

use super::traits::*;
use crate::results::{Result, ResultType};
use anyhow::Result as AnyhowResult;
use std::collections::HashMap;

/// arXiv search engine for scientific papers
pub struct ArXiv {
    api_url: String,
}

impl ArXiv {
    pub fn new() -> Self {
        Self {
            api_url: "https://export.arxiv.org/api/query".to_string(),
        }
    }

    /// Parse the Atom XML response
    fn parse_atom_response(&self, xml: &str) -> Vec<Result> {
        let mut results = Vec::new();

        // Simple XML parsing without a full XML library
        // Look for <entry> elements
        let mut position = 1u32;

        for entry_str in xml.split("<entry>").skip(1) {
            let entry_end = match entry_str.find("</entry>") {
                Some(pos) => pos,
                None => continue,
            };
            let entry = &entry_str[..entry_end];

            // Extract title
            let title = Self::extract_tag(entry, "title")
                .map(|t| t.replace('\n', " ").trim().to_string())
                .unwrap_or_default();

            if title.is_empty() {
                continue;
            }

            // Extract URL (id)
            let url = Self::extract_tag(entry, "id").unwrap_or_default();

            if url.is_empty() {
                continue;
            }

            // Extract summary/abstract
            let abstract_text = Self::extract_tag(entry, "summary")
                .map(|s| s.replace('\n', " ").trim().to_string());

            // Extract authors
            let authors: Vec<String> = entry
                .split("<author>")
                .skip(1)
                .filter_map(|author_block| Self::extract_tag(author_block, "name"))
                .collect();

            let author_str = if authors.is_empty() {
                None
            } else {
                Some(authors.join(", "))
            };

            // Extract PDF link
            let pdf_url = if entry.contains("title=\"pdf\"") {
                // Find the href in the pdf link
                let pdf_start = entry.find("title=\"pdf\"");
                if let Some(start) = pdf_start {
                    let before = &entry[..start];
                    let href_start = before.rfind("href=\"");
                    if let Some(hs) = href_start {
                        let href_content = &before[hs + 6..];
                        let href_end = href_content.find('"');
                        href_end.map(|he| href_content[..he].to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Extract categories/tags
            let categories: Vec<String> = entry
                .split("<category term=\"")
                .skip(1)
                .filter_map(|cat| cat.find('"').map(|end| cat[..end].to_string()))
                .collect();

            // Extract published date
            let published = Self::extract_tag(entry, "published");

            // Build result
            let mut result = Result::new(url, title, self.name().to_string());
            result.result_type = ResultType::Paper;
            result = result.with_position(position);

            if let Some(abstract_text) = abstract_text {
                // Truncate abstract if too long (char-boundary safe)
                let truncated = if abstract_text.len() > 300 {
                    let end = abstract_text.floor_char_boundary(300);
                    format!("{}...", &abstract_text[..end])
                } else {
                    abstract_text
                };
                result = result.with_content(truncated);
            }

            result.metadata.author = author_str;
            result.metadata.published_date = published;
            result.metadata.template = Some("paper.html".to_string());

            // Store PDF URL in file_type field (repurposed)
            if let Some(pdf) = pdf_url {
                result.metadata.file_type = Some(format!("PDF: {}", pdf));
            }

            // Store categories
            if !categories.is_empty() {
                result.category = Some(categories.join(", "));
            }

            results.push(result);
            position += 1;
        }

        results
    }

    /// Extract text content from an XML tag
    fn extract_tag(xml: &str, tag: &str) -> Option<String> {
        let start_tag = format!("<{}", tag);
        let end_tag = format!("</{}>", tag);

        let start = xml.find(&start_tag)?;
        let content_start = xml[start..].find('>')? + start + 1;
        let end = xml[content_start..].find(&end_tag)? + content_start;

        Some(xml[content_start..end].to_string())
    }
}

impl Default for ArXiv {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for ArXiv {
    fn name(&self) -> &str {
        "arxiv"
    }

    fn about(&self) -> EngineAbout {
        EngineAbout::new()
            .website("https://arxiv.org")
            .official_api(true)
            .results_format("XML-RSS")
    }

    fn categories(&self) -> Vec<&str> {
        vec!["science", "scientific publications"]
    }

    fn supports_paging(&self) -> bool {
        true
    }

    fn request(&self, params: &RequestParams) -> AnyhowResult<EngineRequest> {
        let mut query_params = HashMap::new();

        // Build search query with "all:" prefix to search all fields
        query_params.insert("search_query".to_string(), format!("all:{}", params.query));

        // Pagination
        let start = (params.pageno - 1) * 10;
        query_params.insert("start".to_string(), start.to_string());
        query_params.insert("max_results".to_string(), "10".to_string());

        let mut request = EngineRequest::get(&self.api_url);
        request.params = query_params;

        Ok(request)
    }

    fn response(&self, response: EngineResponse) -> AnyhowResult<EngineResults> {
        if !response.is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status));
        }

        let results = self.parse_atom_response(&response.text);

        Ok(EngineResults::with_results(results))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arxiv_request() {
        let arxiv = ArXiv::new();
        let params = RequestParams::new("machine learning");
        let request = arxiv.request(&params).unwrap();

        assert!(request.url.contains("arxiv.org"));
        assert!(request.params.contains_key("search_query"));
        assert_eq!(
            request.params.get("search_query"),
            Some(&"all:machine learning".to_string())
        );
    }

    #[test]
    fn test_extract_tag() {
        let xml = "<entry><title>Test Title</title><summary>Abstract text</summary></entry>";
        assert_eq!(
            ArXiv::extract_tag(xml, "title"),
            Some("Test Title".to_string())
        );
        assert_eq!(
            ArXiv::extract_tag(xml, "summary"),
            Some("Abstract text".to_string())
        );
        assert_eq!(ArXiv::extract_tag(xml, "missing"), None);
    }
}
