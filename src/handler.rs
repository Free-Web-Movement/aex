use std::{ any::{ Any, TypeId }, collections::HashMap, sync::Arc };

use crate::{ protocol::method::HttpMethod, req::Request, res::Response };

// HTTP 上下文
pub struct HTTPContext<'a> {
    pub req: Request<'a>,
    pub res: Response<'a>,
    pub global: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    pub local: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

pub type Executor = fn(&mut HTTPContext) -> bool;

// 保存参数名和 executor 的结构
#[derive(Clone)]
pub struct HandlerMapValue {
    pub parameters: Vec<String>,
    pub executors: Vec<Arc<Executor>>,
}

impl HandlerMapValue {
    pub fn new() -> Self {
        Self {
            parameters: vec![],
            executors: vec![],
        }
    }
}

// 每个路径对应的 handler
#[derive(Clone)]
pub struct Handler {
    pub methods: HashMap<HttpMethod, HandlerMapValue>, // method -> executor 集合
    pub fallback: HandlerMapValue, // 无 method 指定时
}

impl Handler {
    pub fn new() -> Self {
        Self {
            methods: HashMap::new(),
            fallback: HandlerMapValue::new(),
        }
    }

    /// 添加单个 executor
    pub fn add(
        &mut self,
        params: &mut Vec<String>,
        method: Option<HttpMethod>,
        executor: Arc<Executor>
    ) -> &mut Self {
        match method {
            Some(m) => {
                let entry = self.methods.entry(m).or_insert_with(HandlerMapValue::new);
                entry.parameters = params.clone();
                entry.executors.push(executor);
            }
            None => {
                self.fallback.parameters.append(params);
                self.fallback.executors.push(executor);
            }
        }
        self
    }

    /// 添加一组 executor
    pub fn add_vec(
        &mut self,
        params: &mut Vec<String>,
        method: Option<HttpMethod>,
        executors: Vec<Arc<Executor>>
    ) -> &mut Self {
        match method {
            Some(m) => {
                let entry = self.methods.entry(m).or_insert_with(HandlerMapValue::new);
                entry.parameters = params.clone();
                entry.executors.extend(executors);
            }
            None => {
                self.fallback.parameters.append(params);
                self.fallback.executors.extend(executors);
            }
        }
        self
    }

    /// 获取指定 method 的 executor，如果没有则返回 fallback
    pub fn get_executors(&self, method: Option<&HttpMethod>) -> &Vec<Arc<Executor>> {
        match method {
            Some(m) => {
                self.methods
                    .get(m)
                    .map(|v| &v.executors)
                    .unwrap_or(&self.fallback.executors)
            }
            None => &self.fallback.executors,
        }
    }
}
