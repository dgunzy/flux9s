//! Data source connectors for plugins
//!
//! Provides connectors for fetching data from various sources:
//! - Kubernetes Services (most common)
//! - Kubernetes CRDs
//! - External HTTP APIs
//! - Local files (for testing)

mod connector;
mod http;
mod k8s_service;
mod file;

pub use connector::{DataSourceConnector, create_connector};
pub use http::HttpDataSource;
pub use k8s_service::KubernetesServiceDataSource;
pub use file::FileDataSource;
