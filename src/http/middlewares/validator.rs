use std::{collections::HashMap, sync::Arc};
use zz_validator::{
    ast::{FieldRule, FieldType, Value},
    parser::Parser,
    validator::validate_object,
};

use crate::{
    exe,
    http::{params::Params, protocol::status::StatusCode, types::Executor},
};

/// 核心优化点 1：基于引用的转换，避免不必要的 String 拷贝
/// 使用 eq_ignore_ascii_case 替代 to_lowercase() 减少内存分配
fn convert_by_type(s: &str, field_type: &FieldType) -> Value {
    match field_type {
        FieldType::Int => s
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::String(s.to_owned())),

        FieldType::Bool => {
            if s.eq_ignore_ascii_case("true") || s == "1" || s.eq_ignore_ascii_case("on") {
                Value::Bool(true)
            } else if s.eq_ignore_ascii_case("false") || s == "0" || s.eq_ignore_ascii_case("off") {
                Value::Bool(false)
            } else {
                Value::String(s.to_owned())
            }
        }

        FieldType::Float => s
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::String(s.to_owned())),

        _ => Value::String(s.to_owned()),
    }
}

/// 核心优化点 2：统一转换入口，直接从各种原始 Map 中提取
/// 使用 with_capacity 减少 HashMap 扩容开销
fn to_value_optimized<'a, I>(iter_provider: I, rules: &[FieldRule]) -> Value
where
    I: Fn(&str) -> Option<Vec<&'a str>>,
{
    let mut obj = HashMap::with_capacity(rules.len());

    for rule in rules {
        let field_name = &rule.field;
        if let Some(values) = iter_provider(field_name) {
            if rule.is_array {
                let converted = values
                    .iter()
                    .map(|&s| convert_by_type(s, &rule.field_type))
                    .collect();
                obj.insert(field_name.clone(), Value::Array(converted));
            } else if let Some(&first_val) = values.first() {
                obj.insert(
                    field_name.clone(),
                    convert_by_type(first_val, &rule.field_type),
                );
            }
        }
    }
    Value::Object(obj)
}

pub fn to_validator(dsl_map: HashMap<String, String>) -> Arc<Executor> {
    // -----------------------------
    // 1️⃣ 注册期：预解析并将 HashMap 转为 Vec
    // 遍历 Vec<(K,V)> 比遍历 HashMap 性能更好，因为 Source 通常只有 3 个
    // -----------------------------
    let mut compiled_vec = Vec::new();
    for (source, dsl) in dsl_map {
        if !dsl.trim().is_empty() {
            if let Ok(rules) = Parser::parse_rules(&dsl) {
                compiled_vec.push((source, rules));
            }
        }
    }
    let compiled = Arc::new(compiled_vec);

    exe!(|ctx, data| { data }, |ctx| {
        let compiled = compiled.clone();
        let mut res = true;

        let meta = &mut ctx.meta_in;
        println!("Validating request: {} {}", meta.method.to_str(), meta.path);
        let params = meta.params.clone();
        let params = params.unwrap_or_else(|| {
            println!("No params found in metadata, using empty Params.");
            Params::new("".to_string())
        });

        for (source, rules) in compiled.as_ref() {
            // 核心优化点 3：使用闭包作为 Provider，消除中间 HashMap 构造
            let mut value = match source.as_str() {
                "params" => to_value_optimized(
                    |key| {
                        params
                            .data
                            .as_ref()
                            .and_then(|m| m.get(key))
                            .map(|v| vec![v.as_str()])
                    },
                    rules,
                ),
                "body" => to_value_optimized(
                    |key| {
                        params
                            .form
                            .as_ref()
                            .and_then(|m| m.get(key))
                            .map(|v| v.iter().map(|s| s.as_str()).collect())
                    },
                    rules,
                ),
                "query" => to_value_optimized(
                    |key| {
                        params
                            .query
                            .get(key)
                            .map(|v| v.iter().map(|s| s.as_str()).collect())
                    },
                    rules,
                ),
                _ => continue,
            };

            // 2️⃣ 执行校验
            if let Err(e) = validate_object(&mut value, rules) {
                meta.status = StatusCode::BadRequest;
                // 预分配字符串容量
                let mut err_msg = String::with_capacity(64);
                err_msg.push_str(source);
                err_msg.push_str(" validate error: ");
                err_msg.push_str(&e.to_string());

                meta.body = err_msg.as_bytes().to_vec();
                res = false;
                break;
            }
        }
        res
    })
}
