use regex::Regex;
use std::collections::HashMap;

use crate::validator::ast::{ Constraint, FieldRule, FieldType, Value };


/// -----------------------------
/// Validator
/// -----------------------------
pub fn validate_field(value: &mut Value, rule: &FieldRule) -> Result<(), String> {
    // 对对象，先填充默认值
    if let Value::Object(obj) = value {
        if !obj.contains_key(&rule.field) {
            if let Some(d) = &rule.default {
                obj.insert(rule.field.clone(), d.clone());
            }
        }
    }

    // 获取值
    let val_opt = match value {
        Value::Object(obj) => obj.get_mut(&rule.field),
        _ => Some(value),
    };

    if val_opt.is_none() {
        if rule.required {
            return Err(format!("Missing required field {}", rule.field));
        } else {
            return Ok(());
        }
    }

    let val = val_opt.unwrap();

    // union types 验证
    if let Some(types) = &rule.union_types {
        let mut ok = false;
        for t in types {
            if validate_type(val, t).is_ok() {
                ok = true;
                break;
            }
        }
        if !ok {
            return Err(format!(
                "{} value {:?} does not match union types {:?}",
                rule.field, val, types
            ));
        }
    } else {
        validate_type(val, &rule.field_type)?;
    }

    // enum 验证
    if let Some(enum_vals) = &rule.enum_values {
        if !enum_vals.contains(val) {
            return Err(format!("{} value {:?} not in enum {:?}", rule.field, val, enum_vals));
        }
    }

    // constraints 验证
    if let Some(c) = &rule.constraints {
        for con in &c.items {
            match con {
                Constraint::Range { min, max, min_inclusive, max_inclusive } => {
                    match val {
                        Value::Int(i) => {
                            let n = *i as f64;
                            let min_v = match min {
                                Value::Int(mi) => *mi as f64,
                                Value::Float(mf) => *mf,
                                _ => return Err(format!("Invalid min value type in range for {}", rule.field)),
                            };
                            let max_v = match max {
                                Value::Int(mi) => *mi as f64,
                                Value::Float(mf) => *mf,
                                _ => return Err(format!("Invalid max value type in range for {}", rule.field)),
                            };
                            let min_ok = if *min_inclusive { n >= min_v } else { n > min_v };
                            let max_ok = if *max_inclusive { n <= max_v } else { n < max_v };
                            if !min_ok || !max_ok {
                                return Err(format!("{} value {:?} out of range [{:?}, {:?}]", rule.field, val, min, max));
                            }
                        }
                        Value::Float(f) => {
                            let n = *f;
                            let min_v = match min {
                                Value::Int(mi) => *mi as f64,
                                Value::Float(mf) => *mf,
                                _ => return Err(format!("Invalid min value type in range for {}", rule.field)),
                            };
                            let max_v = match max {
                                Value::Int(mi) => *mi as f64,
                                Value::Float(mf) => *mf,
                                _ => return Err(format!("Invalid max value type in range for {}", rule.field)),
                            };
                            let min_ok = if *min_inclusive { n >= min_v } else { n > min_v };
                            let max_ok = if *max_inclusive { n <= max_v } else { n < max_v };
                            if !min_ok || !max_ok {
                                return Err(format!("{} value {:?} out of range [{:?}, {:?}]", rule.field, val, min, max));
                            }
                        }
                        Value::String(s) => {
                            let n = s.len();
                            // min/max 可以是 Value::Int 或 Value::String
                            let min_v = match min {
                                Value::Int(mi) => *mi as usize,
                                Value::String(s) => s.parse::<usize>().map_err(|_| format!("Failed to parse '{}' as usize", s))?,
                                _ => return Err(format!("Invalid min value type in range for {}", rule.field)),
                            };
                            let max_v = match max {
                                Value::Int(mi) => *mi as usize,
                                Value::String(s) => s.parse::<usize>().map_err(|_| format!("Failed to parse '{}' as usize", s))?,
                                _ => return Err(format!("Invalid max value type in range for {}", rule.field)),
                            };
                            let min_ok = if *min_inclusive { n >= min_v } else { n > min_v };
                            let max_ok = if *max_inclusive { n <= max_v } else { n < max_v };
                            if !min_ok || !max_ok {
                                return Err(format!("{} length {} out of range [{:?}, {:?}]", rule.field, n, min, max));
                            }
                        }
                        _ => {
                            return Err(format!("{} cannot apply range constraint to {:?}", rule.field, val));
                        }
                    }
                }
                Constraint::Regex(pattern) => {
                    let s = val.as_str().ok_or(format!("{} not string for regex", rule.field))?;
                    let re = Regex::new(pattern).map_err(|e| format!("Invalid regex: {}", e))?;
                    if !re.is_match(s) {
                        return Err(format!("{} regex mismatch: {}", rule.field, pattern));
                    }
                }
            }
        }
    }

    // sub_rule / array / object 递归验证
    if let Some(sub_rule) = &rule.rule {
        match val {
            Value::Object(_) => validate_field(val, sub_rule)?,
            Value::Array(arr) => {
                for v in arr.iter_mut() {
                    validate_field(v, sub_rule)?;
                }
            }
            _ => {}
        }
    }

    if let Some(children) = &rule.children {
        if let Value::Object(_) = val {
            for child_rule in children {
                validate_field(val, child_rule)?;
            }
        } else {
            return Err(format!("{} is not object but has children", rule.field));
        }
    }

    Ok(())
}

fn validate_type(value: &Value, t: &FieldType) -> Result<(), String> {
    match t {
        FieldType::String => if value.as_str().is_some() { Ok(()) } else { Err("Not string".into()) },
        FieldType::Int => if value.as_int().is_some() { Ok(()) } else { Err("Not int".into()) },
        FieldType::Float => if value.as_float().is_some() { Ok(()) } else { Err("Not float".into()) },
        FieldType::Bool => if value.as_bool().is_some() { Ok(()) } else { Err("Not bool".into()) },
        FieldType::Object => if value.as_object().is_some() { Ok(()) } else { Err("Not object".into()) },
        FieldType::Array => if value.as_array().is_some() { Ok(()) } else { Err("Not array".into()) },
    }
}

pub fn validate_object(value: &mut Value, rules: &[FieldRule]) -> Result<(), String> {
    if let Value::Object(_) = value {
        for rule in rules {
            validate_field(value, rule)?;
        }
        Ok(())
    } else {
        Err("Value is not object".into())
    }
}
#[cfg(test)]
mod validator_tests {
    use super::*;
    use crate::validator::parser::parse_rules;

    #[test]
    fn test_validator_full_dsl() {
        let dsl =
            r#"
        (
            username:string[3,20] regex("^[a-zA-Z0-9_]+$"),
            age:int[0,150]=30,
            score:float(0,100),
            active:bool=true,
            nickname?:string[0,20],
            role:string enum("admin","user","guest")=user,
            id:int|float,
            profile:object(
                first_name:string[1,50],
                last_name:string[1,50],
                contact:object(
                    email:string regex("^[^@\\s]+@[^@\\s]+\\.[^@\\s]+$"),
                    phone?:string[0,20]
                )
            ),
            tags:array<string[1,10]>,
            distance:float[1.0e0,2.0e0]=1.5e0
        )
        "#;

        let rules = parse_rules(dsl).expect("Failed to parse DSL");

        let mut obj = Value::Object(Default::default());

        // 填充合法值
        obj.as_object_mut()
            .unwrap()
            .insert("username".to_string(), Value::String("user_123".to_string()));
        obj.as_object_mut().unwrap().insert("age".to_string(), Value::Int(25));
        obj.as_object_mut().unwrap().insert("score".to_string(), Value::Float(85.5));
        obj.as_object_mut().unwrap().insert("active".to_string(), Value::Bool(true));
        obj.as_object_mut().unwrap().insert("role".to_string(), Value::String("admin".to_string()));
        obj.as_object_mut().unwrap().insert("id".to_string(), Value::Int(101));

        // profile 对象
        let mut profile = Value::Object(Default::default());
        profile
            .as_object_mut()
            .unwrap()
            .insert("first_name".to_string(), Value::String("John".to_string()));
        profile
            .as_object_mut()
            .unwrap()
            .insert("last_name".to_string(), Value::String("Doe".to_string()));
        let mut contact = Value::Object(Default::default());
        contact
            .as_object_mut()
            .unwrap()
            .insert("email".to_string(), Value::String("john@example.com".to_string()));
        profile.as_object_mut().unwrap().insert("contact".to_string(), contact);
        obj.as_object_mut().unwrap().insert("profile".to_string(), profile);

        // tags 数组
        obj.as_object_mut()
            .unwrap()
            .insert(
                "tags".to_string(),
                Value::Array(
                    vec![Value::String("tag1".to_string()), Value::String("tag2".to_string())]
                )
            );

        // 调用 validator
        let res = validate_object(&mut obj, &rules);
        assert!(res.is_ok(), "Validation failed: {:?}", res.err());

        // 默认值填充
        assert_eq!(obj.as_object().unwrap().get("distance"), Some(&Value::Float(1.5)));

        // 错误测试 - 类型不匹配
        let mut bad_obj = obj.clone();
        bad_obj
            .as_object_mut()
            .unwrap()
            .insert("age".to_string(), Value::String("not_a_number".to_string()));
        let err = validate_object(&mut bad_obj, &rules).unwrap_err();
        println!("err = {:?}", err);
        assert!(err.contains("age value"), "Expected age type error, got {}", err);

        // 错误测试 - enum 不匹配
        let mut bad_enum = obj.clone();
        bad_enum
            .as_object_mut()
            .unwrap()
            .insert("role".to_string(), Value::String("superuser".to_string()));
        let err = validate_object(&mut bad_enum, &rules).unwrap_err();
        assert!(err.contains("role value"), "Expected role enum error, got {}", err);

        // 错误测试 - regex 不匹配
        let mut bad_regex = obj.clone();
        bad_regex
            .as_object_mut()
            .unwrap()
            .insert("username".to_string(), Value::String("!!invalid!!".to_string()));
        let err = validate_object(&mut bad_regex, &rules).unwrap_err();
        assert!(err.contains("username regex mismatch"), "Expected regex error, got {}", err);

        // 错误测试 - range 不匹配
        let mut bad_range = obj.clone();
        bad_range.as_object_mut().unwrap().insert("score".to_string(), Value::Float(150.0));
        let err = validate_object(&mut bad_range, &rules).unwrap_err();
        assert!(err.contains("score value"), "Expected range error, got {}", err);
    }
}
