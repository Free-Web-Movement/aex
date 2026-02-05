use std::{ any::{ Any, TypeId }, collections::HashMap };

use crate::{ req::Request, res::Response };

// HTTP 上下文
pub struct HTTPContext {
    pub req: Request,
    pub res: Response,
    pub global: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    pub local: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

pub type Executor = fn(&mut HTTPContext) -> bool;
