//! HTTP request handlers

use super::state::AppState;
use crate::mcp::{McpError, McpRequest, McpResponse};
use crate::query::ParsedQuery;
use crate::search::{EngineRef, SearchQuery};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tera::Context;
use url::Url;

/// Query parameters for search
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    /// Search query
    pub q: Option<String>,
    /// Categories (comma-separated)
    pub categories: Option<String>,
    /// Engines (comma-separated)
    pub engines: Option<String>,
    /// Language
    pub language: Option<String>,
    /// Time range
    #[allow(dead_code)]
    pub time_range: Option<String>,
    /// Safe search level
    pub safesearch: Option<u8>,
    /// Page number
    pub pageno: Option<u32>,
    /// Output format
    pub format: Option<String>,
}

/// Search results response for JSON format
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub number_of_results: usize,
    pub results: Vec<ResultResponse>,
    pub answers: Vec<AnswerResponse>,
    pub suggestions: Vec<String>,
    pub corrections: Vec<String>,
    pub infoboxes: Vec<serde_json::Value>,
    pub unresponsive_engines: Vec<UnresponsiveEngineResponse>,
    pub engine_errors: Vec<EngineError>,
}

/// Engine error information
#[derive(Debug, Serialize)]
pub struct EngineError {
    pub engine: String,
    pub error: String,
    pub error_type: ErrorType,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    ApiKeyRequired,
    RateLimited,
    NetworkError,
    ParseError,
    Timeout,
    Other,
}

/// Unresponsive engine information
#[derive(Debug, Serialize)]
pub struct UnresponsiveEngineResponse {
    pub engine: String,
    pub error: String,
}

/// A single search result in JSON response
#[derive(Debug, Serialize)]
pub struct ResultResponse {
    pub url: String,
    pub title: String,
    pub content: Option<String>,
    pub engine: String,
    pub engines: Vec<String>,
    pub positions: Vec<u32>,
    pub score: f64,
    pub category: Option<String>,
    pub template: Option<String>,
    pub img_src: Option<String>,
    pub thumbnail: Option<String>,
    pub priority: Option<String>,
    pub published_date: Option<String>,
    pub author: Option<String>,
    pub file_type: Option<String>,
    pub file_size: Option<String>,
    pub duration: Option<String>,
    pub views: Option<u64>,
    pub iframe_src: Option<String>,
    pub audio_src: Option<String>,
    pub is_official: bool,
    pub version: Option<String>,
    pub license: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source_code: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub last_update: Option<String>,
    pub architecture: Option<String>,
    pub package_name: Option<String>,
    pub stars: Option<u64>,
    pub forks: Option<u64>,
    pub verified: Option<bool>,
    pub severity: Option<String>,
    pub cvss_score: Option<f64>,
    pub modified_date: Option<String>,
    pub archived: Option<bool>,
    pub mirror: Option<bool>,
    pub trending_score: Option<f64>,
    pub library: Option<String>,
    pub pipeline: Option<String>,
    pub private: Option<bool>,
    pub disabled: Option<bool>,
    pub result_type: String,
    pub parsed_url: ParsedUrlArray,
}

/// Parsed URL as array [scheme, domain, path, query, fragment, extra]
#[derive(Debug, Serialize)]
pub struct ParsedUrlArray(pub Vec<String>);

impl From<&Url> for ParsedUrlArray {
    fn from(url: &Url) -> Self {
        Self(vec![
            url.scheme().to_string(),
            url.host_str().unwrap_or("").to_string(),
            url.path().to_string(),
            url.query().unwrap_or("").to_string(),
            url.fragment().unwrap_or("").to_string(),
            "".to_string(),
        ])
    }
}

/// Answer result in JSON response
#[derive(Debug, Serialize)]
pub struct AnswerResponse {
    pub answer: String,
    pub engine: String,
    pub url: Option<String>,
}

/// Home page handler
pub async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let mut ctx = Context::new();
    ctx.insert("instance_name", state.instance_name());
    ctx.insert("version", crate::VERSION);
    if let Some(name) = state.branding_name() {
        ctx.insert("brand_name", name);
    }
    if let Some(tagline) = state.branding_tagline() {
        ctx.insert("brand_tagline", tagline);
    }
    if let Some(color) = state.branding_accent_color() {
        ctx.insert("accent_color", color);
    }
    if let Some(url) = state.branding.logo.as_ref() {
        ctx.insert("logo_url", url);
    }
    if let Some(data_uri) = state.branding.logo_data_uri.as_ref() {
        ctx.insert("logo_data_uri", data_uri);
    }
    ctx.insert(
        "categories",
        &["general", "images", "videos", "news", "it", "science"],
    );

    match state.templates.render_with_context("index.html", &ctx) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

/// Search handler
pub async fn search(State(state): State<AppState>, Query(params): Query<SearchParams>) -> Response {
    // Check for query
    let raw_query = match params.q {
        Some(q) if !q.trim().is_empty() => q,
        _ => return Redirect::to("/").into_response(),
    };

    // Parse query
    let parsed = ParsedQuery::parse(&raw_query);

    // Build engine refs from categories or engines
    let engine_refs = if let Some(ref engines) = params.engines {
        engines
            .split(',')
            .map(|e| EngineRef::new(e.trim(), "general"))
            .collect()
    } else {
        let categories = params
            .categories
            .as_deref()
            .unwrap_or("general")
            .split(',')
            .map(|c| c.trim())
            .collect::<Vec<_>>();

        categories
            .iter()
            .flat_map(|cat| {
                state
                    .registry
                    .get_by_category(cat)
                    .into_iter()
                    .map(|e| EngineRef::new(e.name(), *cat))
            })
            .collect()
    };

    // Build search query
    let mut search_query = SearchQuery::from_parsed(parsed.clone(), engine_refs);
    search_query.pageno = params.pageno.unwrap_or(1);

    if let Some(ref lang) = params.language {
        search_query.lang = lang.clone();
    }

    if let Some(safesearch) = params.safesearch {
        search_query.safesearch = safesearch;
    }

    // Execute search
    let results = state.search.execute(&search_query).await;

    // Check for redirect
    if let Some(redirect_url) = results.get_redirect() {
        return Redirect::to(&redirect_url).into_response();
    }

    // Check for redirect to first result
    if search_query.redirect_to_first {
        let ordered = results.get_ordered_results();
        if let Some(first) = ordered.first() {
            return Redirect::to(&first.url).into_response();
        }
    }

    // Log any engine errors for debugging
    for unresponsive in results.get_unresponsive() {
        tracing::warn!(
            "Engine '{}' was unresponsive: {:?}",
            unresponsive.name,
            unresponsive.error
        );
    }

    // Format response based on requested format
    match params.format.as_deref() {
        Some("json") => {
            let ordered = results.get_ordered_results();

            // Convert unresponsive engines to detailed error information
            let engine_errors: Vec<EngineError> = results
                .get_unresponsive()
                .iter()
                .map(|ue| EngineError {
                    engine: ue.name.clone(),
                    error: ue.error.to_string(),
                    error_type: match ue.error {
                        crate::results::EngineError::MissingApiKey => ErrorType::ApiKeyRequired,
                        crate::results::EngineError::RateLimited => ErrorType::RateLimited,
                        crate::results::EngineError::NetworkError => ErrorType::NetworkError,
                        crate::results::EngineError::ParseError => ErrorType::ParseError,
                        crate::results::EngineError::Timeout => ErrorType::Timeout,
                        _ => ErrorType::Other,
                    },
                })
                .collect();

            let response = SearchResponse {
                query: raw_query,
                number_of_results: ordered.len(),
                results: ordered
                    .into_iter()
                    .map(|r| {
                        let parsed_url = r.parsed_url.as_ref().map(ParsedUrlArray::from);
                        let result_type_str = match r.result_type {
                            crate::results::ResultType::Default => "default",
                            crate::results::ResultType::Image => "image",
                            crate::results::ResultType::Video => "video",
                            crate::results::ResultType::Map => "map",
                            crate::results::ResultType::News => "news",
                            crate::results::ResultType::Paper => "paper",
                            crate::results::ResultType::File => "file",
                            crate::results::ResultType::Code => "code",
                            crate::results::ResultType::Answer => "answer",
                            crate::results::ResultType::InfoBox => "infobox",
                            crate::results::ResultType::Security => "security",
                            crate::results::ResultType::Corporate => "corporate",
                        };
                        ResultResponse {
                            url: r.url,
                            title: r.title,
                            content: r.content,
                            engine: r.engine,
                            engines: r.engines.into_iter().collect(),
                            positions: r.positions,
                            score: r.score,
                            category: r.category,
                            template: r.metadata.template,
                            img_src: r.metadata.img_src,
                            thumbnail: r.metadata.thumbnail,
                            priority: None, // Could be added if needed
                            published_date: r.metadata.published_date,
                            author: r.metadata.author,
                            file_type: r.metadata.file_type,
                            file_size: r.metadata.file_size,
                            duration: r.metadata.duration,
                            views: r.metadata.views,
                            iframe_src: r.metadata.iframe_src,
                            audio_src: r.metadata.audio_src,
                            is_official: r.metadata.is_official,
                            version: r.metadata.version,
                            license: r.metadata.license,
                            tags: r.metadata.tags,
                            source_code: r.metadata.source_code,
                            homepage: r.metadata.homepage,
                            documentation: r.metadata.documentation,
                            last_update: r.metadata.last_update,
                            architecture: r.metadata.architecture,
                            package_name: r.metadata.package_name,
                            stars: r.metadata.stars,
                            forks: r.metadata.forks,
                            verified: r.metadata.verified,
                            severity: r.metadata.severity,
                            cvss_score: r.metadata.cvss_score,
                            modified_date: r.metadata.modified_date,
                            archived: r.metadata.archived,
                            mirror: r.metadata.mirror,
                            trending_score: r.metadata.trending_score,
                            library: r.metadata.library,
                            pipeline: r.metadata.pipeline,
                            private: r.metadata.private,
                            disabled: r.metadata.disabled,
                            result_type: result_type_str.to_string(),
                            parsed_url: parsed_url.unwrap_or_else(|| ParsedUrlArray(vec![
                                "".to_string(),
                                "".to_string(),
                                "".to_string(),
                                "".to_string(),
                                "".to_string(),
                                "".to_string(),
                            ])),
                        }
                    })
                    .collect(),
                answers: results
                    .get_answers()
                    .into_iter()
                    .map(|a| AnswerResponse {
                        answer: a.answer,
                        engine: a.engine,
                        url: a.url,
                    })
                    .collect(),
                suggestions: results
                    .get_suggestions()
                    .into_iter()
                    .map(|s| s.text)
                    .collect(),
                corrections: vec![], // Could be added if needed
                infoboxes: vec![], // Could be populated if needed
                unresponsive_engines: results
                    .get_unresponsive()
                    .into_iter()
                    .map(|e| UnresponsiveEngineResponse {
                        engine: e.name,
                        error: e.error.to_string(),
                    })
                    .collect(),
                engine_errors,
            };
            Json(response).into_response()
        }
        Some("csv") => {
            let ordered = results.get_ordered_results();
            let mut csv = String::from("title,url,content,engine\n");
            for r in ordered {
                csv.push_str(&format!(
                    "\"{}\",\"{}\",\"{}\",\"{}\"\n",
                    r.title.replace('"', "\"\""),
                    r.url.replace('"', "\"\""),
                    r.content.unwrap_or_default().replace('"', "\"\""),
                    r.engine
                ));
            }
            ([(axum::http::header::CONTENT_TYPE, "text/csv")], csv).into_response()
        }
        _ => {
            // HTML response
            let ordered = results.get_ordered_results();

            let mut ctx = Context::new();
            ctx.insert("instance_name", state.instance_name());
            ctx.insert("version", crate::VERSION);
            if let Some(name) = state.branding_name() {
                ctx.insert("brand_name", name);
            }
            if let Some(tagline) = state.branding_tagline() {
                ctx.insert("brand_tagline", tagline);
            }
            if let Some(color) = state.branding_accent_color() {
                ctx.insert("accent_color", color);
            }
if let Some(url) = state.branding.logo.as_ref() {
        ctx.insert("logo_url", url);
    }
    if let Some(data_uri) = state.branding.logo_data_uri.as_ref() {
        ctx.insert("logo_data_uri", data_uri);
    }
            ctx.insert("query", &raw_query);
            ctx.insert("results", &ordered);
            ctx.insert("answers", &results.get_answers());
            ctx.insert("suggestions", &results.get_suggestions());
            ctx.insert("infoboxes", &results.get_infoboxes());
            ctx.insert("unresponsive_engines", &results.get_unresponsive());
            ctx.insert("timings", &results.get_timings());
            ctx.insert("result_count", &results.result_count());
            ctx.insert("pageno", &search_query.pageno);
            ctx.insert(
                "categories",
                &["general", "images", "videos", "news", "it", "science"],
            );

            // Add engine errors to context for display
            let engine_errors: Vec<(String, String)> = results
                .get_unresponsive()
                .iter()
                .map(|ue| (ue.name.clone(), ue.error.to_string()))
                .collect();
            ctx.insert("engine_errors", &engine_errors);

            match state.templates.render_with_context("search.html", &ctx) {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    tracing::error!("Template error: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
                }
            }
        }
    }
}

/// About page handler
pub async fn about(State(state): State<AppState>) -> impl IntoResponse {
    let mut ctx = Context::new();
    ctx.insert("instance_name", state.instance_name());
    ctx.insert("version", crate::VERSION);
    ctx.insert("engines", &state.registry.names());

    match state.templates.render_with_context("about.html", &ctx) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>About</h1><p>SearXNG-RS</p>".to_string())
        }
    }
}

/// Usage page handler
pub async fn usage(State(state): State<AppState>) -> impl IntoResponse {
    let mut ctx = Context::new();
    ctx.insert("instance_name", state.instance_name());
    ctx.insert("version", crate::VERSION);

    match state.templates.render_with_context("usage.html", &ctx) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Usage Guide</h1><p>Query syntax and API documentation.</p>".to_string())
        }
    }
}

/// Preferences page handler
pub async fn preferences(State(state): State<AppState>) -> impl IntoResponse {
    let mut ctx = Context::new();
    ctx.insert("instance_name", state.instance_name());
    ctx.insert("themes", &state.settings.ui.themes);
    ctx.insert("engines", &state.registry.names());
    ctx.insert("categories", &state.registry.category_names());

    match state
        .templates
        .render_with_context("preferences.html", &ctx)
    {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Preferences</h1>".to_string())
        }
    }
}

/// Stats page handler
pub async fn stats(State(state): State<AppState>) -> impl IntoResponse {
    let mut ctx = Context::new();
    ctx.insert("instance_name", state.instance_name());
    ctx.insert("engines", &state.registry.names());
    ctx.insert("engine_count", &state.registry.len());

    match state.templates.render_with_context("stats.html", &ctx) {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!("Template error: {}", e);
            Html("<h1>Stats</h1>".to_string())
        }
    }
}

/// Health check handler
pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": crate::VERSION
    }))
}

/// SearXNG-compatible instance configuration.
/// Clients like Cherry Studio call this to discover enabled engines:
/// `{ "engines": [{ "name", "enabled", "categories" }] }`
pub async fn config(State(state): State<AppState>) -> impl IntoResponse {
    let mut names = state.registry.names();
    names.sort();
    let engines: Vec<serde_json::Value> = names
        .iter()
        .filter_map(|name| {
            state.registry.get(name).map(|engine| {
                let categories: Vec<String> =
                    engine.categories().iter().map(|s| s.to_string()).collect();
                serde_json::json!({
                    "name": name,
                    "enabled": true,
                    "categories": categories,
                })
            })
        })
        .collect();
    Json(serde_json::json!({ "engines": engines }))
}

/// Autocomplete handler
#[derive(Debug, Deserialize)]
pub struct AutocompleteParams {
    pub q: String,
    /// Override autocomplete backend (optional)
    pub backend: Option<String>,
}

pub async fn autocomplete(
    State(state): State<AppState>,
    Query(params): Query<AutocompleteParams>,
) -> impl IntoResponse {
    // Get the backend to use (from query param or config)
    let backend_name = params
        .backend
        .as_deref()
        .or_else(|| state.autocomplete_backend())
        .unwrap_or("duckduckgo");

    // Get language from settings
    let lang = &state.settings.ui.default_locale;

    // Fetch suggestions
    let suggestions =
        crate::autocomplete::fetch_suggestions(&state.http_client, backend_name, &params.q, lang)
            .await
            .unwrap_or_default();

    // Return in OpenSearch format: [query, [suggestions...]]
    Json(vec![
        serde_json::Value::String(params.q),
        serde_json::Value::Array(
            suggestions
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ),
    ])
}

/// Robots.txt handler
pub async fn robots_txt(State(state): State<AppState>) -> impl IntoResponse {
    let content = if state.is_public() {
        "User-agent: *\nAllow: /\nDisallow: /search\nDisallow: /preferences\n"
    } else {
        "User-agent: *\nDisallow: /\n"
    };
    ([(axum::http::header::CONTENT_TYPE, "text/plain")], content)
}

/// Favicon SVG data (SearXNG logo)
const FAVICON_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 92 92"><g transform="translate(-40.921 -17.417)"><circle cx="75.921" cy="53.903" r="30" style="fill:none;stroke:#3050ff;stroke-width:10"/><path d="M67.515 37.915a18 18 0 0 1 21.051 3.313 18 18 0 0 1 3.138 21.078" style="fill:none;stroke:#3050ff;stroke-width:5"/><rect width="18.846" height="39.963" x="3.706" y="122.09" ry="0" style="fill:#3050ff" transform="rotate(-46.235)"/></g></svg>"#;

/// Favicon handler
pub async fn favicon() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        FAVICON_SVG,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsed_url_array_serialization() {
        let parsed_url = ParsedUrlArray(vec![
            "https".to_string(),
            "example.com".to_string(),
            "/path".to_string(),
            "query=1".to_string(),
            "fragment".to_string(),
            "".to_string(),
        ]);

        let serialized = serde_json::to_string(&parsed_url).unwrap();
        assert_eq!(serialized, r#"["https","example.com","/path","query=1","fragment",""]"#);
    }

    #[test]
    fn test_result_response_has_all_required_fields() {
        let result = ResultResponse {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            content: Some("Content".to_string()),
            engine: "duckduckgo".to_string(),
            engines: vec!["duckduckgo".to_string()],
            positions: vec![1],
            score: 1.0,
            category: Some("general".to_string()),
            template: Some("default.html".to_string()),
            img_src: None,
            thumbnail: None,
            priority: Some("".to_string()),
            published_date: None,
            author: None,
            file_type: None,
            file_size: None,
            duration: None,
            views: None,
            iframe_src: None,
            audio_src: None,
            is_official: false,
            version: None,
            license: None,
            tags: None,
            source_code: None,
            homepage: None,
            documentation: None,
            last_update: None,
            architecture: None,
            package_name: None,
            stars: None,
            forks: None,
            verified: None,
            severity: None,
            cvss_score: None,
            modified_date: None,
            archived: None,
            mirror: None,
            trending_score: None,
            library: None,
            pipeline: None,
            private: None,
            disabled: None,
            result_type: "default".to_string(),
            parsed_url: ParsedUrlArray(vec![
                "https".to_string(),
                "example.com".to_string(),
                "/".to_string(),
                "".to_string(),
                "".to_string(),
                "".to_string(),
            ]),
        };

        let serialized = serde_json::to_value(&result).unwrap();
        
        assert!(serialized.get("url").is_some());
        assert!(serialized.get("title").is_some());
        assert!(serialized.get("content").is_some());
        assert!(serialized.get("engine").is_some());
        assert!(serialized.get("engines").is_some());
        assert!(serialized.get("positions").is_some());
        assert!(serialized.get("score").is_some());
        assert!(serialized.get("category").is_some());
        assert!(serialized.get("template").is_some());
        assert!(serialized.get("img_src").is_some());
        assert!(serialized.get("thumbnail").is_some());
        assert!(serialized.get("priority").is_some());
        assert!(serialized.get("parsed_url").is_some());
        
        let parsed_url = serialized.get("parsed_url").unwrap();
        assert!(parsed_url.is_array());
        assert_eq!(parsed_url.as_array().unwrap().len(), 6);
    }

    #[test]
    fn test_search_response_structure() {
        let response = SearchResponse {
            query: "test".to_string(),
            number_of_results: 10,
            results: vec![],
            answers: vec![],
            suggestions: vec![],
            corrections: vec![],
            infoboxes: vec![],
            unresponsive_engines: vec![],
            engine_errors: vec![],
        };

        let serialized = serde_json::to_value(&response).unwrap();
        
        assert!(serialized.get("query").is_some());
        assert!(serialized.get("number_of_results").is_some());
        assert!(serialized.get("results").is_some());
        assert!(serialized.get("answers").is_some());
        assert!(serialized.get("suggestions").is_some());
        assert!(serialized.get("corrections").is_some());
        assert!(serialized.get("infoboxes").is_some());
        assert!(serialized.get("unresponsive_engines").is_some());
        assert!(serialized.get("engine_errors").is_some());
    }
}

/// MCP POST handler - receives JSON-RPC requests
pub async fn mcp_post(
    State(_state): State<AppState>,
    Json(request): Json<McpRequest>,
) -> impl IntoResponse {
    // Note: For full MCP support, we'd need to pass AppState to handle requests
    // This is a placeholder that returns an error for now
    // The actual implementation would be in the stdio mode or via a different route
    tracing::warn!("MCP HTTP request received but not fully implemented yet");
    
    let response = McpResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: None,
        error: Some(McpError::new(
            -32000,
            "MCP HTTP endpoint is experimental. Use stdio mode (--mcp) for full support.".to_string(),
        )),
    };
    
    Json(response)
}
