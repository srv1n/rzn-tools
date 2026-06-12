# Apple Health Connector - NOT READY FOR USE

## Status: Experimental / Disabled

This connector is **disabled** and excluded from builds. The code is preserved for future use.

## Why It Doesn't Work

While Apple added HealthKit framework support to macOS 14.0+ (Sonoma), the actual Health **data store** is not available on Mac:

```
HKHealthStore::isHealthDataAvailable() → false
```

### What This Means

- **HealthKit APIs are callable** - The framework loads and methods can be invoked
- **No Health app on Mac** - Unlike iPhone/iPad, macOS has no Health app
- **No local data store** - Health data lives on iPhone/Apple Watch, synced via iCloud
- **Authorization will fail** - Even if you could request access, there's no data to access

### Apple's Documentation

From [Apple Developer Documentation](https://developer.apple.com/documentation/healthkit):
> "HealthKit provides a central repository for health and fitness data on iPhone and Apple Watch."

macOS is listed as supported (14.0+) but only for API availability, not data availability.

## When This Might Change

Apple could potentially:
1. Add a Health app to macOS in a future release
2. Enable iCloud Health data access on Mac
3. Support Mac Catalyst apps with Health entitlements

Monitor WWDC announcements for updates.

## To Re-Enable (When Ready)

1. **Uncomment in `rzn_tools_core/Cargo.toml`:**
   ```toml
   apple-health = ["dep:objc2-health-kit", "dep:objc2-foundation", "dep:objc2", "dep:block2"]
   ```
   And add `"apple-health"` to `all-connectors`.

2. **Uncomment in `rzn_tools_core/src/connectors/mod.rs`:**
   ```rust
   #[cfg(all(target_os = "macos", feature = "apple-health"))]
   pub mod apple_health;
   ```

3. **Uncomment in `rzn_tools_core/src/lib.rs`:**
   ```rust
   #[cfg(all(target_os = "macos", feature = "apple-health"))]
   {
       let connector = connectors::apple_health::AppleHealthConnector::new();
       registry.register_provider(Box::new(connector));
   }
   ```

4. **Uncomment in `rzn_tools_cli/Cargo.toml`:**
   ```toml
   apple-health = ["rzn_tools_core/apple-health"]
   ```

5. **Test availability:**
   ```bash
   cargo build --features apple-health
   ./target/debug/rzn-tools tools apple-health

   # This connector does not yet have a dedicated CLI wrapper command.
   # When testing via MCP, use `tools/call` with the tool name `check_availability`.
   ```

## Current Implementation

The connector (`mod.rs`) implements:
- `check_availability` - Calls `HKHealthStore::isHealthDataAvailable()`
- `list_supported_types` - Lists health data types (steps, heart_rate, etc.)
- `get_authorization_status` - Checks auth status for a data type

These work but return "unavailable" since there's no data store.

## Alternative Approaches

If you need health data on Mac, consider:

1. **Health Export XML** - Users can export from iPhone Health app to XML
2. **Fitness platform APIs** - Strava, Garmin Connect, Fitbit have REST APIs
3. **iOS Companion App** - Build an iOS app that syncs data to a server

## Dependencies (When Enabled)

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2-health-kit = { version = "0.2", features = ["all"] }
objc2-foundation = { version = "0.2", features = [...] }
objc2 = { version = "0.5" }
block2 = { version = "0.5" }
```

---

*Last tested: December 2024 on macOS Sequoia (15.x)*
