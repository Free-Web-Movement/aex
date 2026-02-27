use std::{collections::HashMap, sync::Arc};
use zz_validator::{
    ast::{FieldRule, FieldType, Value},
    parser::Parser,
    validator::validate_object,
};

use crate::{
    connection::context::TypeMapExt,
    exe,
    http::{meta::HttpMetadata, protocol::status::StatusCode, types::Executor},
};

/// 1. 独立转换函数：确保在 to_value_optimized 作用域内可见
/// 失败时返回 String 类型的错误描述，供中间件回写 Body
fn convert_by_type(s: &str, field_type: &FieldType) -> Result<Value, String> {
    let res = match field_type {
        FieldType::Int => s
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| format!("'{}' is not a valid integer", s)),

        FieldType::Bool => {
            // 严格匹配逻辑
            if s.eq_ignore_ascii_case("true") || s == "1" || s.eq_ignore_ascii_case("on") {
                Ok(Value::Bool(true))
            } else if s.eq_ignore_ascii_case("false") || s == "0" || s.eq_ignore_ascii_case("off") {
                Ok(Value::Bool(false))
            } else {
                // 命中该分支即报错，解决了测试不到 fallback 的问题
                Err(format!("'{}' is not a valid boolean", s))
            }
        }

        FieldType::Float => s
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| format!("'{}' is not a valid float", s)),

        // String 类型及其他默认走这里
        _ => Ok(Value::String(s.to_owned())),
    };
    res
}

/// 2. 优化后的值收集函数
/// 返回 Result 以确保能够使用 ? 操作符进行短路返回（报错即停止）
fn to_value_optimized<'a, I>(iter_provider: I, rules: &[FieldRule]) -> Result<Value, String>
where
    I: Fn(&str) -> Option<Vec<&'a str>>,
{
    let mut obj = HashMap::with_capacity(rules.len());

    for rule in rules {
        let field_name = &rule.field;
        if let Some(values) = iter_provider(field_name) {
            if rule.is_array {
                // 修复 E0277 核心：明确显式声明 Result<Vec<Value>, String>
                // 这样 collect 才知道如何将 Result 项聚合为带结果的集合
                let converted: Result<Vec<Value>, String> = values
                    .iter()
                    .map(|&s| convert_by_type(s, &rule.field_type))
                    .collect();

                obj.insert(field_name.clone(), Value::Array(converted?));
            } else if let Some(&first_val) = values.first() {
                // 单个值直接转换并用 ? 向上抛错
                let value = convert_by_type(first_val, &rule.field_type)?;
                obj.insert(field_name.clone(), value);
            }
        }
    }
    Ok(Value::Object(obj))
}

pub fn value_to_string(v: Value) -> String {
    match v {
        Value::Bool(b) => {
            if b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            let s = f.to_string();
            // 🚀 核心修复：如果转换结果没小数点，手动补上，防止校验器认为它是 Int
            if !s.contains('.') {
                format!("{}.0", s)
            } else {
                s
            }
        }
        Value::String(s) => s, // 直接移动所有权，无分配
        _ => "".to_string(),
    }
}

pub fn to_validator(dsl_map: HashMap<String, String>) -> Arc<Executor> {
    // 1️⃣ 注册期：预解析规则
    let mut compiled_vec = Vec::new();
    for (source, dsl) in dsl_map {
        if !dsl.trim().is_empty() {
            match Parser::parse_rules(&dsl) {
                Ok(rules) => {
                    compiled_vec.push((source, rules));
                }
                Err(e) => {
                    eprintln!("❌ DSL Parse Error [{}]: {:?}", source, e);
                }
            }
        }
    }

    let compiled = Arc::new(compiled_vec);

    exe!(|ctx, data| { data }, |ctx| {
        let compiled = compiled.clone();

        // 获取 Metadata，注意：我们需要在校验结束后将其写回
        let mut meta = ctx
            .local
            .get_value::<HttpMetadata>()
            .expect("HttpMetadata missing");

        // 拿到 Params 的副本进行操作
        let mut params = meta
            .params
            .clone()
            .expect("AEX FATAL: HttpMetadata.params container must be pre-initialized by the protocol layer");
        let mut res = true;

        for (source, rules) in compiled.as_ref() {
            // 2️⃣ 执行转换逻辑
            let value_result = match source.as_str() {
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
                _ => {
                    continue;
                }
            };

            // 3️⃣ 处理转换与校验结果
            match value_result {
                Ok(mut value) => {
                    // 执行 zz-validator 校验
                    // 这一步非常关键，它会处理 default 值并验证 logic
                    if let Err(e) = validate_object(&mut value, rules) {
                        let mut err_msg = String::with_capacity(64);
                        err_msg.push_str(source);
                        err_msg.push_str(" validate error: ");
                        err_msg.push_str(&e.to_string());

                        meta.status = StatusCode::BadRequest;
                        meta.body = err_msg.into_bytes();
                        res = false;
                        break;
                    }

                    if let Value::Object(obj) = value {
                        match source.as_str() {
                            "query" => {
                                for (k, v) in obj {
                                    params.query.insert(
                                        k,
                                        match v {
                                            Value::Array(arr) => {
                                                arr.into_iter().map(&value_to_string).collect()
                                            }
                                            _ => vec![value_to_string(v)],
                                        },
                                    );
                                }
                            }
                            "body" => {
                                let form_map = params.form.get_or_insert_with(HashMap::new);
                                for (k, v) in obj {
                                    form_map.insert(
                                        k,
                                        match v {
                                            Value::Array(arr) => {
                                                arr.into_iter().map(&value_to_string).collect()
                                            }
                                            _ => vec![value_to_string(v)],
                                        },
                                    );
                                }
                            }
                            "params" => {
                                let data_map = params.data.get_or_insert_with(HashMap::new);
                                for (k, v) in obj {
                                    data_map.insert(k, value_to_string(v));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(conv_err) => {
                    // 捕获 convert_by_type 抛出的严格错误（无 to_owned 路径）
                    let mut err_msg = String::with_capacity(64);
                    err_msg.push_str(source);
                    err_msg.push_str(" conversion error: ");
                    err_msg.push_str(&conv_err);

                    meta.status = StatusCode::BadRequest;
                    meta.body = err_msg.into_bytes();
                    res = false;
                    break;
                }
            }
        }

        // 4️⃣ 统一写回 Metadata
        // 无论成功还是失败（错误信息和状态码），都必须 set_value 才能生效
        if res {
            meta.params = Some(params);
        }
        ctx.local.set_value(meta);

        res
    })
}
