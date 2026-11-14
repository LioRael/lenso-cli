use std::{future::Future, pin::Pin, sync::Arc};
#[derive(Clone, Debug)]
pub struct Principal { pub subject: String, pub scopes: Vec<String> }
pub type IdentityResult = Result<Option<Principal>, Box<dyn std::error::Error + Send + Sync>>;
pub trait Identity: Send + Sync {
    fn resolve<'a>(&'a self, token: Option<&'a str>) -> Pin<Box<dyn Future<Output=IdentityResult> + Send + 'a>>;
}
#[derive(Clone, Default)]
pub struct Services { pub identity: Option<Arc<dyn Identity>> }
pub struct Mounted { pub router: axum::Router, pub identity: Option<Arc<dyn Identity>> }
