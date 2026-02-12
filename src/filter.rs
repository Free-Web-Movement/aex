use futures::{FutureExt, future::BoxFuture};
use regex::Regex;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use crate::types::{Executor, HTTPContext};

#[derive(Debug, Clone)]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Object,
}

#[derive(Debug, Clone)]
pub struct LengthConstraint {
    pub min: Option<usize>,
    pub max: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ValueConstraint {
    pub min: Option<i64>,
    pub max: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct FloatValueConstraint {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Constraints {
    pub length: Option<LengthConstraint>,
    pub int_value: Option<ValueConstraint>,
    pub float_value: Option<FloatValueConstraint>,
    pub regex: Option<Regex>,
}

#[derive(Debug, Clone)]
pub struct FieldRule {
    pub source: String, // "body", "query", "params"
    pub field: String,
    pub field_type: FieldType,
    pub required: bool,
    pub is_array: bool,
    pub constraints: Option<Constraints>,
    pub fields: Option<Vec<FieldRule>>, // 对象嵌套
}

pub fn validate_field(field_rule: &FieldRule, data: &Value) -> Result<(), String> {
    // 根据 source 获取数据
    let value = data.get(&field_rule.field);

    if value.is_none() {
        if field_rule.required {
            return Err(format!("Field '{}' is required", field_rule.field));
        } else {
            return Ok(());
        }
    }

    let value = value.unwrap();

    if field_rule.is_array {
        if !value.is_array() {
            return Err(format!("Field '{}' should be an array", field_rule.field));
        }

        for v in value.as_array().unwrap() {
            validate_single(field_rule, v)?;
        }
    } else {
        validate_single(field_rule, value)?;
    }

    Ok(())
}

fn validate_single(field_rule: &FieldRule, value: &Value) -> Result<(), String> {
    match field_rule.field_type {
        FieldType::String => {
            if !value.is_string() {
                return Err(format!("Field '{}' must be a string", field_rule.field));
            }
            if let Some(c) = &field_rule.constraints {
                if let Some(len) = &c.length {
                    let len_val = value.as_str().unwrap().len();
                    if let Some(min) = len.min {
                        if len_val < min {
                            return Err(format!("Field '{}' length < {}", field_rule.field, min));
                        }
                    }
                    if let Some(max) = len.max {
                        if len_val > max {
                            return Err(format!("Field '{}' length > {}", field_rule.field, max));
                        }
                    }
                }
                if let Some(regex) = &c.regex {
                    if !regex.is_match(value.as_str().unwrap()) {
                        return Err(format!("Field '{}' regex mismatch", field_rule.field));
                    }
                }
            }
        }
        FieldType::Int => {
            if !value.is_i64() && !value.is_u64() {
                return Err(format!("Field '{}' must be an integer", field_rule.field));
            }
            if let Some(c) = &field_rule.constraints {
                let val = value.as_i64().unwrap();
                if let Some(v) = &c.int_value {
                    if let Some(min) = v.min {
                        if val < min {
                            return Err(format!("Field '{}' < {}", field_rule.field, min));
                        }
                    }
                    if let Some(max) = v.max {
                        if val > max {
                            return Err(format!("Field '{}' > {}", field_rule.field, max));
                        }
                    }
                }
            }
        }
        FieldType::Float => {
            if !value.is_f64() && !value.is_i64() && !value.is_u64() {
                return Err(format!("Field '{}' must be a float", field_rule.field));
            }
            if let Some(c) = &field_rule.constraints {
                let val = value.as_f64().unwrap();
                if let Some(v) = &c.float_value {
                    if let Some(min) = v.min {
                        if val < min {
                            return Err(format!("Field '{}' < {}", field_rule.field, min));
                        }
                    }
                    if let Some(max) = v.max {
                        if val > max {
                            return Err(format!("Field '{}' > {}", field_rule.field, max));
                        }
                    }
                }
            }
        }
        FieldType::Bool => {
            if !value.is_boolean() {
                return Err(format!("Field '{}' must be a boolean", field_rule.field));
            }
        }
        FieldType::Object => {
            if !value.is_object() {
                return Err(format!("Field '{}' must be an object", field_rule.field));
            }
            if let Some(fields) = &field_rule.fields {
                for f in fields {
                    validate_field(f, value)?;
                }
            }
        }
    }

    Ok(())
}

pub fn make_data_filter_executor(
    rules: Arc<Vec<FieldRule>>,
) -> impl Fn(&mut HTTPContext) -> BoxFuture<'_, Result<(), String>> + Send + Sync + 'static {
    move |ctx: &mut HTTPContext| {
        let rules = Arc::new(rules.clone());
        async move {
            for rule in rules.iter() {
                let _value: Option<&[String]> = match rule.source.as_str() {
                    "body" => ctx
                        .req
                        .params
                        .form
                        .as_ref()
                        .and_then(|map| map.get(&rule.field))
                        .map(|vec| vec.as_slice()),

                    "query" => ctx
                        .req
                        .params
                        .query
                        .get(&rule.field)
                        .map(|vec| vec.as_slice()),

                    "data" => ctx
                        .req
                        .params
                        .data
                        .as_ref()
                        .and_then(|map| map.get(&rule.field))
                        .map(|s| std::slice::from_ref(s)), // 包装单值为 slice
                    _ => None,
                };

                // validate_value(rule, value)?;
            }
            Ok(())
        }
        .boxed()
    }
}

/// 校验单个字段，支持 Option<&[String]>
/// value: None 表示字段不存在
pub fn validate_value(rule: &FieldRule, value: Option<&[String]>) -> Result<(), String> {
    // 必填检查
    if rule.required {
        if value.is_none() || value.unwrap().is_empty() {
            return Err(format!("Field '{}' is required", rule.field));
        }
    }

    // 没有值且非必填，直接返回 OK
    let values = match value {
        Some(vs) if !vs.is_empty() => vs,
        _ => return Ok(()),
    };

    // 遍历所有值（支持数组字段）
    for val in values.iter() {
        match rule.field_type {
            FieldType::String => {
                if let Some(constraints) = &rule.constraints {
                    // 长度约束
                    if let Some(len) = &constraints.length {
                        if let Some(min) = len.min {
                            if val.len() < min {
                                return Err(format!(
                                    "Field '{}' length {} < min {}",
                                    rule.field,
                                    val.len(),
                                    min
                                ));
                            }
                        }
                        if let Some(max) = len.max {
                            if val.len() > max {
                                return Err(format!(
                                    "Field '{}' length {} > max {}",
                                    rule.field,
                                    val.len(),
                                    max
                                ));
                            }
                        }
                    }

                    // 正则约束
                    if let Some(re) = &constraints.regex {
                        if !re.is_match(val) {
                            return Err(format!(
                                "Field '{}' value '{}' does not match regex",
                                rule.field, val
                            ));
                        }
                    }
                }
            }
            FieldType::Int => {
                let num: i64 = val.parse().map_err(|_| {
                    format!("Field '{}' value '{}' is not an integer", rule.field, val)
                })?;
                if let Some(constraints) = &rule.constraints {
                    if let Some(value) = &constraints.int_value {
                        if let Some(min) = value.min {
                            if num < min {
                                return Err(format!(
                                    "Field '{}' value {} < min {}",
                                    rule.field, num, min
                                ));
                            }
                        }
                        if let Some(max) = value.max {
                            if num > max {
                                return Err(format!(
                                    "Field '{}' value {} > max {}",
                                    rule.field, num, max
                                ));
                            }
                        }
                    }
                }
            }
            FieldType::Float => {
                let num: f64 = val.parse().map_err(|_| {
                    format!("Field '{}' value '{}' is not a float", rule.field, val)
                })?;
                if let Some(constraints) = &rule.constraints {
                    if let Some(value) = &constraints.float_value {
                        if let Some(min) = value.min {
                            if num < min as f64 {
                                return Err(format!(
                                    "Field '{}' value {} < min {}",
                                    rule.field, num, min
                                ));
                            }
                        }
                        if let Some(max) = value.max {
                            if num > max as f64 {
                                return Err(format!(
                                    "Field '{}' value {} > max {}",
                                    rule.field, num, max
                                ));
                            }
                        }
                    }
                }
            }
            FieldType::Bool => {
                val.parse::<bool>().map_err(|_| {
                    format!("Field '{}' value '{}' is not boolean", rule.field, val)
                })?;
            }
            FieldType::Object => todo!(),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    use serde_json::json;

    #[test]
    fn test_string_field_valid() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "username".to_string(),
            field_type: FieldType::String,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: Some(LengthConstraint {
                    min: Some(2),
                    max: Some(10),
                }),
                int_value: None,
                float_value: None,
                regex: Some(Regex::new(r"^[a-z]+$").unwrap()),
            }),
            fields: None,
        };

        let data = json!({ "username": "abc" });
        assert!(validate_field(&rule, &data).is_ok());
    }

    #[test]
    fn test_string_field_too_short() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "username".to_string(),
            field_type: FieldType::String,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: Some(LengthConstraint {
                    min: Some(3),
                    max: Some(10),
                }),
                int_value: None,
                float_value: None,
                regex: None,
            }),
            fields: None,
        };

        let data = json!({ "username": "ab" });
        assert!(validate_field(&rule, &data).is_err());
    }

    #[test]
    fn test_string_field_regex_fail() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "username".to_string(),
            field_type: FieldType::String,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: None,
                int_value: None,
                float_value: None,
                regex: Some(Regex::new(r"^[a-z]+$").unwrap()),
            }),
            fields: None,
        };

        let data = json!({ "username": "abc123" });
        assert!(validate_field(&rule, &data).is_err());
    }

    #[test]
    fn test_int_field_valid_and_range() {
        let rule = FieldRule {
            source: "query".to_string(),
            field: "age".to_string(),
            field_type: FieldType::Int,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: None,
                float_value: None,
                int_value: Some(ValueConstraint {
                    min: Some(0),
                    max: Some(120),
                }),
                regex: None,
            }),
            fields: None,
        };

        let data = json!({ "age": 30 });
        assert!(validate_field(&rule, &data).is_ok());

        let data2 = json!({ "age": -1 });
        assert!(validate_field(&rule, &data2).is_err());
    }

    #[test]
    fn test_float_field_valid_and_range() {
        let rule = FieldRule {
            source: "query".to_string(),
            field: "score".to_string(),
            field_type: FieldType::Float,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: None,
                int_value: None,
                float_value: Some(FloatValueConstraint {
                    min: Some(0.0),
                    max: Some(100.0),
                }),
                regex: None,
            }),
            fields: None,
        };

        let data = json!({ "score": 88.5 });
        assert!(validate_field(&rule, &data).is_ok());

        let data2 = json!({ "score": 150.0 });
        assert!(validate_field(&rule, &data2).is_err());
    }

    #[test]
    fn test_bool_field() {
        let rule = FieldRule {
            source: "params".to_string(),
            field: "flag".to_string(),
            field_type: FieldType::Bool,
            required: true,
            is_array: false,
            constraints: None,
            fields: None,
        };

        let data = json!({ "flag": true });
        assert!(validate_field(&rule, &data).is_ok());

        let data2 = json!({ "flag": "true" });
        assert!(validate_field(&rule, &data2).is_err());
    }

    #[test]
    fn test_array_field() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "tags".to_string(),
            field_type: FieldType::String,
            required: true,
            is_array: true,
            constraints: Some(Constraints {
                length: Some(LengthConstraint {
                    min: Some(2),
                    max: Some(5),
                }),
                int_value: None,
                float_value: None,
                regex: None,
            }),
            fields: None,
        };

        let data = json!({ "tags": ["ok", "tag2"] });
        assert!(validate_field(&rule, &data).is_ok());

        let data2 = json!({ "tags": ["a", "toolongtag"] });
        assert!(validate_field(&rule, &data2).is_err());
    }

    #[test]
    fn test_object_field_nested() {
        let nested_rule = FieldRule {
            source: "body".to_string(),
            field: "profile".to_string(),
            field_type: FieldType::Object,
            required: true,
            is_array: false,
            constraints: None,
            fields: Some(vec![FieldRule {
                source: "body".to_string(),
                field: "email".to_string(),
                field_type: FieldType::String,
                required: true,
                is_array: false,
                constraints: Some(Constraints {
                    length: None,
                    int_value: None,
                    float_value: None,
                    regex: Some(Regex::new(r"^\S+@\S+\.\S+$").unwrap()),
                }),
                fields: None,
            }]),
        };

        let data = json!({
            "profile": {
                "email": "test@example.com"
            }
        });
        assert!(validate_field(&nested_rule, &data).is_ok());

        let data2 = json!({
            "profile": {
                "email": "invalid-email"
            }
        });
        assert!(validate_field(&nested_rule, &data2).is_err());
    }

    #[test]
    fn test_required_field_missing() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "username".to_string(),
            field_type: FieldType::String,
            required: true,
            is_array: false,
            constraints: None,
            fields: None,
        };

        let data = json!({});
        assert!(validate_field(&rule, &data).is_err());
    }

    #[test]
    fn test_validate_value_string_ok() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: Some(LengthConstraint {
                    min: Some(2),
                    max: Some(5),
                }),
                int_value: None,
                float_value: None,
                regex: Some(Regex::new(r"^[a-z]+$").unwrap()),
            }),
            fields: None,
        };

        let val = Some(&["abc".to_string()][..]);
        assert!(validate_value(&rule, val).is_ok());
    }

    #[test]
    fn test_validate_value_string_fail_length() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: Some(LengthConstraint {
                    min: Some(4),
                    max: Some(5),
                }),
                int_value: None,
                float_value: None,
                regex: None,
            }),
            fields: None,
        };

        let val = Some(&["abc".to_string()][..]);
        assert!(validate_value(&rule, val).is_err());
    }

    #[test]
    fn test_validate_value_int_ok_and_fail() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "age".to_string(),
            field_type: FieldType::Int,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: None,
                int_value: Some(ValueConstraint {
                    min: Some(1),
                    max: Some(100),
                }),
                float_value: None,
                regex: None,
            }),
            fields: None,
        };

        let val_ok = Some(&["30".to_string()][..]);
        let val_fail = Some(&["150".to_string()][..]);

        assert!(validate_value(&rule, val_ok).is_ok());
        assert!(validate_value(&rule, val_fail).is_err());
    }

    #[test]
    fn test_validate_value_float_ok_and_fail() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "score".to_string(),
            field_type: FieldType::Float,
            required: true,
            is_array: false,
            constraints: Some(Constraints {
                length: None,
                int_value: None,
                float_value: Some(FloatValueConstraint {
                    min: Some(0.0),
                    max: Some(100.0),
                }),
                regex: None,
            }),
            fields: None,
        };

        let val_ok = Some(&["88.5".to_string()][..]);
        let val_fail = Some(&["150.0".to_string()][..]);

        assert!(validate_value(&rule, val_ok).is_ok());
        assert!(validate_value(&rule, val_fail).is_err());
    }

    #[test]
    fn test_validate_value_bool_ok_and_fail() {
        let rule = FieldRule {
            source: "body".to_string(),
            field: "flag".to_string(),
            field_type: FieldType::Bool,
            required: true,
            is_array: false,
            constraints: None,
            fields: None,
        };

        let val_ok = Some(&["true".to_string()][..]);
        let val_fail = Some(&["yes".to_string()][..]);

        assert!(validate_value(&rule, val_ok).is_ok());
        assert!(validate_value(&rule, val_fail).is_err());
    }

    #[cfg(test)]
    mod http_integration_tests {
        use crate::req::Request;
        use crate::res::Response;
        use crate::router::{NodeType, Router, handle_request};
        use crate::types::TypeMap;

        use super::*;
        use futures::FutureExt;
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
        use tokio::net::{TcpListener, TcpStream};
        use tokio::sync::Mutex;

        #[tokio::test]
        async fn test_data_filter_executor_in_real_request() {
            // 1️⃣ 构建路由
            let mut root = Router::new(NodeType::Static("root".into()));

            // 定义规则
            let rules = Arc::new(vec![
                FieldRule {
                    source: "body".into(),
                    field: "username".into(),
                    field_type: FieldType::String,
                    required: true,
                    is_array: false,
                    constraints: Some(Constraints {
                        length: Some(LengthConstraint {
                            min: Some(2),
                            max: Some(10),
                        }),
                        int_value: None,
                        float_value: None,
                        regex: Some(regex::Regex::new(r"^[a-z]+$").unwrap()),
                    }),
                    fields: None,
                },
                FieldRule {
                    source: "data".into(),
                    field: "age".into(),
                    field_type: FieldType::Int,
                    required: true,
                    is_array: false,
                    constraints: Some(Constraints {
                        length: None,
                        int_value: Some(ValueConstraint {
                            min: Some(1),
                            max: Some(100),
                        }),
                        float_value: None,
                        regex: None,
                    }),
                    fields: None,
                },
            ]);

            // POST 路由，带数据过滤中间件
            crate::route!(
                root,
                crate::post!("/", {
                    let rules = rules.clone();
                    move |ctx: &mut HTTPContext| {
                        let executor = make_data_filter_executor(rules.clone());
                        Box::pin(async move {
                            // 运行中间件
                            executor(ctx).await.unwrap();

                            ctx.res.body.push("validated".to_string());
                            true
                        })
                        .boxed()
                    }
                })
            );

            // 2️⃣ 起 TCP server
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            tokio::spawn(async move {
                let (stream, peer_addr) = listener.accept().await.unwrap();
                let (reader, writer) = stream.into_split();
                let reader = BufReader::new(reader);
                let writer = BufWriter::new(writer);

                // 3️⃣ 构造请求对象
                let req = Request::new(reader, peer_addr, "")
                    .await
                    .expect("request creation");
                let res = Response::new(writer);
                let mut ctx = HTTPContext {
                    req,
                    res,
                    global: Arc::new(Mutex::new(TypeMap::new())),
                    local: TypeMap::new(),
                };

                // 4️⃣ 执行路由
                handle_request(&root, &mut ctx).await;

                ctx.res.send().await.unwrap();
            });

            // 5️⃣ 客户端发请求
            let mut client = TcpStream::connect(addr).await.unwrap();
            // 请求 body: username=alice
            let request_str =
                b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 13\r\n\r\nusername=alice";
            client.write_all(request_str).await.unwrap();

            // 6️⃣ 读取响应
            let mut resp = vec![0; 1024];
            let n = client.read(&mut resp).await.unwrap();
            let resp_str = std::str::from_utf8(&resp[..n]).unwrap();

            // 7️⃣ 断言：中间件校验通过，响应包含 "validated"
            assert!(resp_str.contains("validated"));
        }
    }
}
