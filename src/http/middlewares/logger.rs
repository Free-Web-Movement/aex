use std::sync::Arc;

use crate::{
    exe,
    http::{meta::HttpMetadata, types::Executor},
};

#[derive(Clone, Default)]
pub struct LogConfig {
    log_method: bool,
    log_path: bool,
    log_user_agent: bool,
}

impl LogConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn log_method(mut self, enable: bool) -> Self {
        self.log_method = enable;
        self
    }

    pub fn log_path(mut self, enable: bool) -> Self {
        self.log_path = enable;
        self
    }

    pub fn log_user_agent(mut self, enable: bool) -> Self {
        self.log_user_agent = enable;
        self
    }

    pub fn all(self) -> Self {
        self.log_method(true).log_path(true).log_user_agent(true)
    }

    pub fn build(self) -> Arc<Executor> {
        let config = self;
        exe!(move |ctx, config| {
            if let Some(meta) = ctx.local.get_ref::<HttpMetadata>() {
                let method = if config.log_method {
                    Some(meta.method.to_str())
                } else {
                    None
                };

                let path = if config.log_path {
                    Some(meta.path.as_str())
                } else {
                    None
                };

                match (method, path) {
                    (Some(m), Some(p)) => {
                        tracing::info!(target: "aex", "{} {} [AEX]", m, p);
                    }
                    (Some(m), None) => {
                        tracing::info!(target: "aex", "{} [AEX]", m);
                    }
                    (None, Some(p)) => {
                        tracing::info!(target: "aex", "{} [AEX]", p);
                    }
                    (None, None) => {}
                }
            }

            true
        }, |ctx| { config.clone() })
    }
}

#[macro_export]
macro_rules! logger {
    () => {
        $crate::http::middlewares::logger::LogConfig::new().all().build()
    };
    ($($t:tt)*) => {
        $crate::http::middlewares::logger::LogConfig::new()$($t)*.build()
    };
}
