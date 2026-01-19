//! Data source connectors for plugins
//!
//! Provides connectors for fetching data from various sources:
//! - Kubernetes Services (most common)
//! - Kubernetes CRDs
//! - External HTTP APIs
//! - Local files (for testing)

mod connector;
mod file;
mod http;
mod k8s_service;

pub use connector::{DataSourceConnector, create_connector};
