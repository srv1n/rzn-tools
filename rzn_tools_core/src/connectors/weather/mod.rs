use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::ingest::{ContentBlock, ContentItem, NormalizedPageV1, OutputFormat, Partial, Source};
use crate::utils::{structured_result, structured_result_with_text};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use reqwest::Client;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;
use urlencoding::encode;

const DEFAULT_FORECAST_DAYS: usize = 3;
const MAX_FORECAST_DAYS: usize = 3;

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum ResponseFormat {
    #[default]
    Concise,
    Detailed,
}

#[derive(Debug, Deserialize)]
struct GetWeatherArgs {
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    days: Option<usize>,
    #[serde(default)]
    units: Option<String>,
    #[serde(default)]
    response_format: ResponseFormat,
    #[serde(default)]
    output_format: OutputFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitSystem {
    Metric,
    Imperial,
}

impl UnitSystem {
    fn parse(raw: Option<&str>) -> Result<Self, ConnectorError> {
        match raw.map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(Self::Metric),
            Some(value) => {
                let normalized = value.to_ascii_lowercase();
                match normalized.as_str() {
                    "metric" | "m" | "si" | "c" | "celsius" => Ok(Self::Metric),
                    "imperial" | "i" | "us" | "f" | "fahrenheit" => Ok(Self::Imperial),
                    _ => Err(ConnectorError::InvalidParams(format!(
                        "Invalid 'units': '{}'. Expected 'metric' or 'imperial'.",
                        value
                    ))),
                }
            }
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Metric => "metric",
            Self::Imperial => "imperial",
        }
    }

    fn temp_unit(self) -> &'static str {
        match self {
            Self::Metric => "C",
            Self::Imperial => "F",
        }
    }

    fn wind_unit(self) -> &'static str {
        match self {
            Self::Metric => "km/h",
            Self::Imperial => "mph",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WttrTextValue {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WttrCurrentCondition {
    #[serde(rename = "temp_C", default)]
    temp_c: String,
    #[serde(rename = "temp_F", default)]
    temp_f: String,
    #[serde(rename = "FeelsLikeC", default)]
    feels_like_c: String,
    #[serde(rename = "FeelsLikeF", default)]
    feels_like_f: String,
    #[serde(default)]
    humidity: String,
    #[serde(rename = "windspeedKmph", default)]
    windspeed_kmph: String,
    #[serde(rename = "windspeedMiles", default)]
    windspeed_miles: String,
    #[serde(rename = "winddir16Point", default)]
    wind_dir_16_point: String,
    #[serde(rename = "precipMM", default)]
    precip_mm: String,
    #[serde(rename = "weatherDesc", default)]
    weather_desc: Vec<WttrTextValue>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WttrArea {
    #[serde(rename = "areaName", default)]
    area_name: Vec<WttrTextValue>,
    #[serde(default)]
    region: Vec<WttrTextValue>,
    #[serde(default)]
    country: Vec<WttrTextValue>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WttrAstronomy {
    #[serde(default)]
    sunrise: String,
    #[serde(default)]
    sunset: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WttrHourly {
    #[serde(rename = "weatherDesc", default)]
    weather_desc: Vec<WttrTextValue>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WttrForecastDay {
    #[serde(default)]
    date: String,
    #[serde(rename = "mintempC", default)]
    min_temp_c: String,
    #[serde(rename = "mintempF", default)]
    min_temp_f: String,
    #[serde(rename = "maxtempC", default)]
    max_temp_c: String,
    #[serde(rename = "maxtempF", default)]
    max_temp_f: String,
    #[serde(rename = "avgtempC", default)]
    avg_temp_c: String,
    #[serde(rename = "avgtempF", default)]
    avg_temp_f: String,
    #[serde(default)]
    astronomy: Vec<WttrAstronomy>,
    #[serde(default)]
    hourly: Vec<WttrHourly>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct WttrResponse {
    #[serde(rename = "current_condition", default)]
    current_condition: Vec<WttrCurrentCondition>,
    #[serde(rename = "nearest_area", default)]
    nearest_area: Vec<WttrArea>,
    #[serde(default)]
    weather: Vec<WttrForecastDay>,
}

pub struct WeatherConnector {
    client: Client,
}

impl WeatherConnector {
    pub async fn new(_auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools-weather-connector/0.1.0")
            .build()
            .map_err(ConnectorError::HttpRequest)?;
        Ok(Self { client })
    }

    fn weather_item_ref(location: &str) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(location.as_bytes());
        format!("weather:report:{}", encoded)
    }

    fn first_non_empty_text(values: &[WttrTextValue]) -> Option<String> {
        values
            .iter()
            .map(|entry| entry.value.trim())
            .find(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }

    fn normalize_location(requested: Option<&str>, response: &WttrResponse) -> String {
        if let Some(area) = response.nearest_area.first() {
            let mut parts = Vec::new();
            if let Some(value) = Self::first_non_empty_text(&area.area_name) {
                parts.push(value);
            }
            if let Some(value) = Self::first_non_empty_text(&area.region) {
                parts.push(value);
            }
            if let Some(value) = Self::first_non_empty_text(&area.country) {
                parts.push(value);
            }
            if !parts.is_empty() {
                return parts.join(", ");
            }
        }

        requested
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "Current location".to_string())
    }

    fn choose_temp<'a>(units: UnitSystem, metric: &'a str, imperial: &'a str) -> Option<&'a str> {
        let candidate = match units {
            UnitSystem::Metric => metric.trim(),
            UnitSystem::Imperial => imperial.trim(),
        };
        if candidate.is_empty() {
            None
        } else {
            Some(candidate)
        }
    }

    fn format_current_summary(condition: &WttrCurrentCondition, units: UnitSystem) -> String {
        let description = Self::first_non_empty_text(&condition.weather_desc)
            .unwrap_or_else(|| "No weather description".to_string());

        let temp = Self::choose_temp(units, &condition.temp_c, &condition.temp_f).unwrap_or("?");
        let feels = Self::choose_temp(units, &condition.feels_like_c, &condition.feels_like_f)
            .unwrap_or("?");
        let humidity = condition.humidity.trim();
        let wind_speed = match units {
            UnitSystem::Metric => condition.windspeed_kmph.trim(),
            UnitSystem::Imperial => condition.windspeed_miles.trim(),
        };
        let wind_dir = condition.wind_dir_16_point.trim();
        let precipitation = condition.precip_mm.trim();

        let mut parts = vec![format!(
            "{}. {}°{} (feels {}°{})",
            description,
            temp,
            units.temp_unit(),
            feels,
            units.temp_unit()
        )];

        if !humidity.is_empty() {
            parts.push(format!("humidity {}%", humidity));
        }
        if !wind_speed.is_empty() {
            if wind_dir.is_empty() {
                parts.push(format!("wind {} {}", wind_speed, units.wind_unit()));
            } else {
                parts.push(format!(
                    "wind {} {} {}",
                    wind_speed,
                    units.wind_unit(),
                    wind_dir
                ));
            }
        }
        if !precipitation.is_empty() {
            parts.push(format!("precip {} mm", precipitation));
        }

        parts.join(", ")
    }

    fn forecast_day_description(day: &WttrForecastDay) -> String {
        day.hourly
            .iter()
            .find_map(|hour| Self::first_non_empty_text(&hour.weather_desc))
            .unwrap_or_else(|| "No description".to_string())
    }

    fn format_forecast_summary(day: &WttrForecastDay, units: UnitSystem) -> String {
        let description = Self::forecast_day_description(day);
        let min_temp =
            Self::choose_temp(units, &day.min_temp_c, &day.min_temp_f).unwrap_or("unknown");
        let max_temp =
            Self::choose_temp(units, &day.max_temp_c, &day.max_temp_f).unwrap_or("unknown");
        let avg_temp =
            Self::choose_temp(units, &day.avg_temp_c, &day.avg_temp_f).unwrap_or("unknown");
        let date = if day.date.trim().is_empty() {
            "unknown date"
        } else {
            day.date.trim()
        };

        let mut summary = format!(
            "{}: {}, min {}°{}, max {}°{}, avg {}°{}",
            date,
            description,
            min_temp,
            units.temp_unit(),
            max_temp,
            units.temp_unit(),
            avg_temp,
            units.temp_unit()
        );

        if let Some(astro) = day.astronomy.first() {
            let sunrise = astro.sunrise.trim();
            let sunset = astro.sunset.trim();
            if !sunrise.is_empty() || !sunset.is_empty() {
                summary.push_str(&format!(", sunrise {}, sunset {}", sunrise, sunset));
            }
        }

        summary
    }

    async fn fetch_weather(
        &self,
        location: Option<&str>,
    ) -> Result<(WttrResponse, Value), ConnectorError> {
        let base_url = match location {
            Some(loc) if !loc.trim().is_empty() => {
                let encoded_location = encode(loc.trim()).into_owned();
                format!("https://wttr.in/{}", encoded_location)
            }
            _ => "https://wttr.in".to_string(),
        };

        let response = self
            .client
            .get(base_url)
            .query(&[("format", "j1")])
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = response.status();
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "wttr.in returned HTTP status {}",
                status
            )));
        }

        let payload: Value = response.json().await.map_err(ConnectorError::HttpRequest)?;
        let parsed: WttrResponse = serde_json::from_value(payload.clone()).map_err(|e| {
            ConnectorError::Other(format!("Failed to parse wttr.in response: {}", e))
        })?;

        Ok((parsed, payload))
    }

    fn normalized_weather_page(
        resolved_location: &str,
        current_summary: &str,
        forecast_summaries: &[String],
        days: usize,
        units: UnitSystem,
        current: Option<&WttrCurrentCondition>,
    ) -> NormalizedPageV1 {
        let item_ref = Self::weather_item_ref(resolved_location);
        let mut blocks = Vec::new();
        blocks.push(ContentBlock {
            block_ref: format!("{}:current", item_ref),
            block_kind: "summary".to_string(),
            text: current_summary.to_string(),
            author: None,
            created_at: None,
            reply_to: None,
            position: Some(json!({ "section": "current" })),
            score: None,
            attachments: Vec::new(),
            metadata: current.map(|condition| {
                json!({
                    "temperature_c": condition.temp_c,
                    "temperature_f": condition.temp_f,
                    "feels_like_c": condition.feels_like_c,
                    "feels_like_f": condition.feels_like_f,
                    "humidity": condition.humidity,
                })
            }),
        });

        for (index, summary) in forecast_summaries.iter().enumerate() {
            blocks.push(ContentBlock {
                block_ref: format!("{}:forecast:{}", item_ref, index),
                block_kind: "forecast_day".to_string(),
                text: summary.clone(),
                author: None,
                created_at: None,
                reply_to: None,
                position: Some(json!({ "section": "forecast", "index": index })),
                score: None,
                attachments: Vec::new(),
                metadata: None,
            });
        }

        let item = ContentItem {
            item_ref,
            kind: "weather_report".to_string(),
            canonical_url: Some("https://wttr.in".to_string()),
            title: Some(format!("Weather for {}", resolved_location)),
            created_at: None,
            source_updated_at: None,
            authors: Vec::new(),
            tags: vec!["weather".to_string(), units.as_str().to_string()],
            metadata: Some(json!({
                "location": resolved_location,
                "days": days,
                "units": units.as_str(),
            })),
            blocks,
            relationships: Vec::new(),
            truncation: None,
        };

        NormalizedPageV1::new(
            vec![item],
            None,
            false,
            Partial::complete(Some(json!({
                "days": days,
                "units": units.as_str(),
            }))),
            Source::new("weather", "get_weather"),
        )
    }
}

#[async_trait]
impl Connector for WeatherConnector {
    fn name(&self) -> &'static str {
        "weather"
    }

    fn description(&self) -> &'static str {
        "Current weather and short forecast via wttr.in"
    }

    fn display_name(&self) -> &'static str {
        "Weather"
    }

    fn icon(&self) -> &'static str {
        "weather"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["weather", "utilities"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: None,
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Use get_weather to fetch current conditions and a short forecast via wttr.in."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: Vec::new(),
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![Tool {
            name: Cow::Borrowed("get_weather"),
            title: None,
            description: Some(Cow::Borrowed(
                "Get current weather and short forecast using wttr.in. \
Example: location=\"San Francisco\" days=2 units=\"metric\".",
            )),
            input_schema: Arc::new(
                json!({
                    "type": "object",
                    "examples": [
                        {
                            "location": "San Francisco, CA",
                            "days": 2,
                            "units": "imperial",
                            "response_format": "concise",
                            "output_format": "raw"
                        },
                        {
                            "location": "London, UK",
                            "days": 3,
                            "units": "metric",
                            "response_format": "concise",
                            "output_format": "normalized_v1"
                        }
                    ],
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "City, ZIP/postal code, or location query. If omitted, wttr.in uses IP-based location."
                        },
                        "days": {
                            "type": "integer",
                            "description": "Forecast days to include (1-3).",
                            "minimum": 1,
                            "maximum": 3,
                            "default": 3
                        },
                        "units": {
                            "type": "string",
                            "description": "Temperature/wind units.",
                            "enum": ["metric", "imperial"],
                            "default": "metric"
                        },
                        "response_format": {
                            "type": "string",
                            "description": "Concise includes normalized weather fields; detailed also includes raw wttr payload.",
                            "enum": ["concise", "detailed"],
                            "default": "concise"
                        },
                        "output_format": {
                            "type": "string",
                            "description": "Default raw. Use normalized_v1/display_v1 for ingest/display pipelines.",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "default": "raw"
                        }
                    },
                    "additionalProperties": false,
                    "_meta": {
                        "category": "read",
                        "tags": ["weather", "forecast", "utilities"],
                        "auth_required": false,
                        "supports_output_format": true
                    }
                })
                .as_object()
                .expect("tool schema object")
                .clone(),
            ),
            output_schema: None,
            annotations: None,
            icons: None,
        }];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let tool_name = request.name.as_ref();
        let args_map = request.arguments.unwrap_or_default();
        let parsed: GetWeatherArgs = serde_json::from_value(Value::Object(args_map.clone()))
            .map_err(|e| ConnectorError::InvalidParams(format!("Invalid arguments: {}", e)))?;

        match tool_name {
            "get_weather" | "current" => {
                let requested_location = parsed
                    .location
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);

                let days = parsed
                    .days
                    .unwrap_or(DEFAULT_FORECAST_DAYS)
                    .clamp(1, MAX_FORECAST_DAYS);
                let units = UnitSystem::parse(parsed.units.as_deref())?;

                let (response, raw_payload) =
                    self.fetch_weather(requested_location.as_deref()).await?;
                let resolved_location =
                    Self::normalize_location(requested_location.as_deref(), &response);

                let current = response.current_condition.first();
                let current_summary = current
                    .map(|condition| Self::format_current_summary(condition, units))
                    .unwrap_or_else(|| "No current weather conditions available.".to_string());

                let forecast_days: Vec<WttrForecastDay> =
                    response.weather.iter().take(days).cloned().collect();
                let forecast_summaries = forecast_days
                    .iter()
                    .map(|day| Self::format_forecast_summary(day, units))
                    .collect::<Vec<_>>();

                if parsed.output_format.is_normalized() || parsed.output_format.is_display() {
                    let page = Self::normalized_weather_page(
                        &resolved_location,
                        &current_summary,
                        &forecast_summaries,
                        days,
                        units,
                        current,
                    );
                    return structured_result(&page);
                }

                let mut data = json!({
                    "provider": "wttr.in",
                    "location": resolved_location,
                    "requested_location": requested_location,
                    "units": units.as_str(),
                    "days": days,
                    "current_summary": current_summary,
                    "forecast_summaries": forecast_summaries,
                    "current": current,
                    "forecast": forecast_days,
                });

                if parsed.response_format == ResponseFormat::Detailed {
                    data["raw"] = raw_payload;
                }

                structured_result_with_text(&data, None)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: Vec::new(),
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(format!(
            "Prompt '{}' not found",
            name
        )))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Vec<WttrTextValue> {
        vec![WttrTextValue {
            value: value.to_string(),
        }]
    }

    #[test]
    fn parses_units_aliases() {
        assert_eq!(
            UnitSystem::parse(None).expect("default units"),
            UnitSystem::Metric
        );
        assert_eq!(
            UnitSystem::parse(Some("metric")).expect("metric units"),
            UnitSystem::Metric
        );
        assert_eq!(
            UnitSystem::parse(Some("imperial")).expect("imperial units"),
            UnitSystem::Imperial
        );
        assert!(UnitSystem::parse(Some("kelvin")).is_err());
    }

    #[test]
    fn normalizes_location_from_nearest_area() {
        let response = WttrResponse {
            current_condition: Vec::new(),
            nearest_area: vec![WttrArea {
                area_name: text("San Francisco"),
                region: text("California"),
                country: text("United States of America"),
            }],
            weather: Vec::new(),
        };

        let location = WeatherConnector::normalize_location(Some("sf"), &response);
        assert_eq!(
            location,
            "San Francisco, California, United States of America"
        );
    }

    #[test]
    fn formats_current_summary_in_selected_units() {
        let condition = WttrCurrentCondition {
            temp_c: "12".to_string(),
            temp_f: "54".to_string(),
            feels_like_c: "10".to_string(),
            feels_like_f: "50".to_string(),
            humidity: "70".to_string(),
            windspeed_kmph: "18".to_string(),
            windspeed_miles: "11".to_string(),
            wind_dir_16_point: "NW".to_string(),
            precip_mm: "0.0".to_string(),
            weather_desc: text("Partly cloudy"),
        };

        let metric = WeatherConnector::format_current_summary(&condition, UnitSystem::Metric);
        assert!(metric.contains("12°C"));
        assert!(metric.contains("18 km/h"));

        let imperial = WeatherConnector::format_current_summary(&condition, UnitSystem::Imperial);
        assert!(imperial.contains("54°F"));
        assert!(imperial.contains("11 mph"));
    }
}
