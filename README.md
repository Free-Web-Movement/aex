# aex


```
use std::{collections::HashMap, sync::Arc};
use crate::trie::{TrieNode, NodeType, handle_request};
use crate::handler::HTTPContext;

let mut root = TrieNode::new(NodeType::Static("root".into()));

// 注册静态路径 GET 处理器
root.insert(
    "/hello",
    Some("GET"),
    Arc::new(|ctx| async move {
        ctx.res.body.push("world");
        true
    }.boxed()),
    None,
);
```