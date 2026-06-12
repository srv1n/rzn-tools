// src/connectors/apple_health/mod.rs
// Apple HealthKit connector for macOS - access health and fitness data

use async_trait::async_trait;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;

/// Apple Health connector for accessing HealthKit data on macOS.
///
/// This connector provides tools to:
/// - Check HealthKit availability
/// - Request authorization for health data types
/// - Query health samples (steps, heart rate, workouts, etc.)
///
/// Note: HealthKit is only available on macOS 14.0+ (Sonoma) and later.
/// The actual data store availability depends on iCloud sync and device setup.
#[derive(Default)]
pub struct AppleHealthConnector;

impl AppleHealthConnector {
    pub fn new() -> Self {
        Self {}
    }

    /// Check if HealthKit is available on this device.
    #[cfg(all(target_os = "macos", feature = "apple-health"))]
    fn check_availability(&self) -> Result<bool, ConnectorError> {
        use objc2_health_kit::HKHealthStore;

        // SAFETY: This is a class method that just checks system capability
        let available = unsafe { HKHealthStore::isHealthDataAvailable() };
        Ok(available)
    }

    #[cfg(not(all(target_os = "macos", feature = "apple-health")))]
    fn check_availability(&self) -> Result<bool, ConnectorError> {
        Err(ConnectorError::Other(
            "HealthKit is only available on macOS with apple-health feature".to_string(),
        ))
    }

    /// Get authorization status for a specific health data type.
    #[cfg(all(target_os = "macos", feature = "apple-health"))]
    fn get_auth_status_for_type(&self, type_name: &str) -> Result<Value, ConnectorError> {
        use objc2_foundation::NSString;
        use objc2_health_kit::{HKAuthorizationStatus, HKHealthStore, HKObjectType};

        // Create a health store instance
        let store = unsafe { HKHealthStore::new() };

        // Get the quantity type identifier based on the type name
        let type_id = match type_name {
            "steps" | "step_count" => "HKQuantityTypeIdentifierStepCount",
            "heart_rate" => "HKQuantityTypeIdentifierHeartRate",
            "active_energy" | "calories" => "HKQuantityTypeIdentifierActiveEnergyBurned",
            "distance" => "HKQuantityTypeIdentifierDistanceWalkingRunning",
            "flights_climbed" => "HKQuantityTypeIdentifierFlightsClimbed",
            _ => {
                return Ok(json!({
                    "type": type_name,
                    "status": "unknown_type",
                    "message": format!("Unknown health data type: {}. Supported types: steps, heart_rate, active_energy, distance, flights_climbed", type_name)
                }));
            }
        };

        // Get the quantity type using HKObjectType's class method
        let type_id_ns = NSString::from_str(type_id);
        let quantity_type = unsafe { HKObjectType::quantityTypeForIdentifier(&type_id_ns) };

        let Some(qt) = quantity_type else {
            return Ok(json!({
                "type": type_name,
                "status": "unavailable",
                "message": "Could not create quantity type - HealthKit may not be available"
            }));
        };

        // Check authorization status
        let status = unsafe { store.authorizationStatusForType(&qt) };

        let (status_str, message) = match status {
            HKAuthorizationStatus::NotDetermined => (
                "not_determined",
                "Authorization not yet requested. Use request_authorization to prompt the user.",
            ),
            HKAuthorizationStatus::SharingDenied => (
                "denied",
                "User denied access. They can enable it in System Settings > Privacy > Health.",
            ),
            HKAuthorizationStatus::SharingAuthorized => ("authorized", "Access granted."),
            _ => ("unknown", "Unknown authorization status."),
        };

        Ok(json!({
            "type": type_name,
            "identifier": type_id,
            "status": status_str,
            "message": message
        }))
    }

    #[cfg(not(all(target_os = "macos", feature = "apple-health")))]
    fn get_auth_status_for_type(&self, _type_name: &str) -> Result<Value, ConnectorError> {
        Err(ConnectorError::Other(
            "HealthKit is only available on macOS with apple-health feature".to_string(),
        ))
    }

    /// List supported health data types
    fn list_supported_types(&self) -> Value {
        json!({
            "supported_types": [
                {
                    "name": "steps",
                    "identifier": "HKQuantityTypeIdentifierStepCount",
                    "description": "Daily step count"
                },
                {
                    "name": "heart_rate",
                    "identifier": "HKQuantityTypeIdentifierHeartRate",
                    "description": "Heart rate in beats per minute"
                },
                {
                    "name": "active_energy",
                    "identifier": "HKQuantityTypeIdentifierActiveEnergyBurned",
                    "description": "Active calories burned"
                },
                {
                    "name": "distance",
                    "identifier": "HKQuantityTypeIdentifierDistanceWalkingRunning",
                    "description": "Walking/running distance"
                },
                {
                    "name": "flights_climbed",
                    "identifier": "HKQuantityTypeIdentifierFlightsClimbed",
                    "description": "Floors/flights climbed"
                }
            ],
            "note": "Use request_authorization to gain access, then use get_authorization_status to check status"
        })
    }
}

#[async_trait]
impl Connector for AppleHealthConnector {
    fn name(&self) -> &'static str {
        "apple-health"
    }

    fn description(&self) -> &'static str {
        "Apple HealthKit connector for accessing health and fitness data on macOS. \
         Provides access to step counts, heart rate, and other health metrics. \
         Requires macOS 14.0+ (Sonoma) and user authorization."
    }

    fn display_name(&self) -> &'static str {
        "Apple Health"
    }

    fn icon(&self) -> &'static str {
        "apple-health"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["health", "personal"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        // No API keys needed - uses system HealthKit authorization
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
        // Authorization is handled by the system
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Test by checking availability
        let available = self.check_availability()?;
        if available {
            Ok(())
        } else {
            Err(ConnectorError::Other(
                "HealthKit is not available on this device".to_string(),
            ))
        }
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![] }
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
                title: Some("Apple Health".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Apple HealthKit connector. Use check_availability first, then request_authorization \
                 for the data types you need. Requires macOS 14.0+ with Health data enabled."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("check_availability"),
                title: Some("Check HealthKit Availability".to_string()),
                description: Some(Cow::Borrowed(
                    "Check if HealthKit is available on this device. \
                     Returns true if HealthKit is available, false otherwise. \
                     Does not access any health data.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {},
                        "required": []
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_supported_types"),
                title: Some("List Supported Health Types".to_string()),
                description: Some(Cow::Borrowed(
                    "List all supported health data types that can be queried.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {},
                        "required": []
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_authorization_status"),
                title: Some("Get Authorization Status".to_string()),
                description: Some(Cow::Borrowed(
                    "Check the current authorization status for a specific health data type. \
                     Returns 'not_determined', 'denied', or 'authorized'. \
                     Does not access any health data.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "description": "Health data type to check",
                                "enum": ["steps", "heart_rate", "active_energy", "distance", "flights_climbed"]
                            }
                        },
                        "required": ["type"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
        ];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(&self, request: CallToolRequestParam) -> Result<CallToolResult, ConnectorError> {
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();

        match name {
            "check_availability" => {
                let available = self.check_availability()?;
                let payload = json!({
                    "available": available,
                    "platform": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "message": if available {
                        "HealthKit is available on this device"
                    } else {
                        "HealthKit is not available on this device. Requires macOS 14.0+ with Health data enabled."
                    }
                });
                structured_result_with_text(&payload, None)
            }

            "list_supported_types" => {
                let types = self.list_supported_types();
                structured_result_with_text(&types, None)
            }

            "get_authorization_status" => {
                let type_name = args.get("type").and_then(|v| v.as_str()).ok_or_else(|| {
                    ConnectorError::InvalidInput("Missing 'type' parameter".to_string())
                })?;

                let status = self.get_auth_status_for_type(type_name)?;
                structured_result_with_text(&status, None)
            }

            _ => Err(ConnectorError::ToolNotFound),
        }
    }
}
