//! Auth dependency-inversion seam shared between `xai-file-utils`
//! (the holder) and `xai-grok-shell` (the implementer). Keeps shell types
//! out of data-collector's import graph while still letting refresh-aware
//! token resolution drive HTTP requests.

pub mod auth_provider;
#[cfg(feature = "middleware")]
pub mod retry_middleware;
pub mod visibility;
pub mod credential_store;
pub mod model_store;
pub mod models_api;
pub mod oauth;

pub use auth_provider::{AuthCredentialProvider, CredentialSnapshot, StaticAuthCredentialProvider};
#[cfg(feature = "middleware")]
pub use retry_middleware::AuthRetryMiddleware;
pub use visibility::HttpAuth;
pub use credential_store::{Credential, CredentialStore, CredentialStoreError, FileCredentialStore};
pub use model_store::{FileModelStore, ModelStore, StoredModels};
pub use models_api::fetch_models;
pub use oauth::flow::{login_oauth, OAuthFlowConfig};
