use crate::{
    exe,
    http::{
        meta::HttpMetadata, protocol::header::HeaderKey, protocol::status::StatusCode,
        types::Executor,
    },
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct CorsConfig {
    allow_origin_all: bool,
    allow_methods: Vec<String>,
    allow_headers: Vec<String>,
    allow_credentials: bool,
    max_age: Option<usize>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allow_origin_all: true,
            allow_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "OPTIONS".to_string(),
            ],
            allow_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-Requested-With".to_string(),
            ],
            allow_credentials: true,
            max_age: Some(86400),
        }
    }
}

impl CorsConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_origin_all(mut self, allow: bool) -> Self {
        self.allow_origin_all = allow;
        self
    }

    pub fn allow_methods(mut self, methods: Vec<&str>) -> Self {
        self.allow_methods = methods.into_iter().map(|s| s.to_uppercase()).collect();
        self
    }

    pub fn allow_headers(mut self, headers: Vec<&str>) -> Self {
        self.allow_headers = headers.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }

    pub fn max_age(mut self, seconds: usize) -> Self {
        self.max_age = Some(seconds);
        self
    }

    pub fn build(self) -> Arc<Executor> {
        let config = Arc::new(self);
        exe!(move |ctx, config| {
            let mut is_options = false;
            
            if let Some(meta) = ctx.local.get_mut::<HttpMetadata>() {
                let origin = meta.headers.get(&HeaderKey::Origin).cloned();

                if origin.is_some() || config.allow_origin_all {
                    let origin_value = origin.as_deref().unwrap_or("*");
                    meta.headers.insert(
                        HeaderKey::AccessControlAllowOrigin,
                        origin_value.to_string(),
                    );
                }

                meta.headers.insert(
                    HeaderKey::AccessControlAllowMethods,
                    config.allow_methods.join(", "),
                );
                meta.headers.insert(
                    HeaderKey::AccessControlAllowHeaders,
                    config.allow_headers.join(", "),
                );

                if config.allow_credentials {
                    meta.headers
                        .insert(HeaderKey::AccessControlAllowCredentials, "true".to_string());
                }

                if let Some(max_age) = config.max_age {
                    meta.headers
                        .insert(HeaderKey::AccessControlMaxAge, max_age.to_string());
                }

                if meta.method.to_str() == "OPTIONS" {
                    is_options = true;
                }
            }

            if is_options {
                ctx.status(StatusCode::Ok).send("", None);
                return false;
            }

            true
        }, |ctx| { config.clone() })
    }
}

#[macro_export]
macro_rules! cors {
    () => {
        $crate::http::middlewares::cors::CorsConfig::new().build()
    };
    ($($t:tt)*) => {
        $crate::http::middlewares::cors::CorsConfig::new()$($t)*.build()
    };
}
