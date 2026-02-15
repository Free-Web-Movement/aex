use std::{ collections::HashMap, sync::Arc };

use zz_validator::{ ast::{FieldRule, FieldType}, parser::Parser, validator::validate_object };

use crate::{ exe, protocol::status::StatusCode, types::Executor };

use zz_validator::ast::Value;


fn to_value_with_rules(
    map: HashMap<String, Vec<String>>, 
    rules: &[FieldRule]
) -> Value {
    let mut obj = HashMap::new();

    for rule in rules {
        let field_name = &rule.field;
        
        // 从 Map 中获取对应的原始字符串数组
        if let Some(values) = map.get(field_name) {
            if rule.is_array {
                // 如果规则是数组，转换所有元素
                let converted_values: Vec<Value> = values
                    .iter()
                    .map(|s| convert_by_type(s, &rule.field_type))
                    .collect();
                obj.insert(field_name.clone(), Value::Array(converted_values));
            } else if let Some(first_val) = values.first() {
                // 如果是非数组，只取第一个值并转换
                obj.insert(field_name.clone(), convert_by_type(first_val, &rule.field_type));
            }
        }
    }

    Value::Object(obj)
}

/// 根据规则定义的类型进行强制转换
fn convert_by_type(s: &str, field_type: &FieldType) -> Value {
    match field_type {
        FieldType::Int => s.parse::<i64>()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::String(s.to_string())), // 转换失败回退到 String，让后面的校验器报错
        
        FieldType::Bool => match s.to_lowercase().as_str() {
            "true" | "1" | "on" => Value::Bool(true),
            "false" | "0" | "off" => Value::Bool(false),
            _ => Value::String(s.to_string()),
        },
        
        FieldType::Float => s.parse::<f64>()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::String(s.to_string())),

        // 默认作为字符串处理
        _ => Value::String(s.to_string()),
    }
}

pub fn to_validator(dsl_map: HashMap<String, String>) -> Arc<Executor> {
    // -----------------------------
    // 注册期：解析 DSL
    // -----------------------------
    let mut compiled_map: HashMap<String, _> = HashMap::new();
    for (key, dsl) in dsl_map {
        if !dsl.trim().is_empty() {
            let rules = Parser::parse_rules(&dsl).unwrap();
            compiled_map.insert(key, rules);
        }
    }

    let compiled = Arc::new(compiled_map);

    exe!(
        |ctx, data| { data },
        |ctx| {
            let mut res = true;
            let compiled = compiled.clone();

            for (source, rules) in compiled.iter() {
                let data = match source.as_str() {
                    // params: Option<HashMap<String, String>>
                    "params" => {
                        ctx.req.params.data
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|(k, v)| (k, vec![v]))
                            .collect()
                    }

                    // form: Option<HashMap<String, Vec<String>>>
                    "body" => { ctx.req.params.form.clone().unwrap_or_default() }

                    // query: Option<HashMap<String, String>>
                    "query" => { ctx.req.params.query.clone() }

                    _ => {
                        continue;
                    }
                };
                let mut value = to_value_with_rules(data, rules);

                if let Err(e) = validate_object(&mut value, rules) {
                    ctx.res.status = StatusCode::BadRequest;
                    let str = format!("{} validate error: {}", source, e);
                    ctx.res.body.push(str);
                    res = false;
                    break;
                }
            }
            res
        }
    )
}
