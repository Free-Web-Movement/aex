use std::collections::HashMap;

/// -----------------------------
/// AST
/// -----------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Object,
    Array,
}

#[derive(Debug, Clone)]
pub enum Constraint {
    Range {
        min: Value,
        max: Value,
        min_inclusive: bool,
        max_inclusive: bool,
    },
    Regex(String),
}

#[derive(Debug, Clone)]
pub struct Constraints {
    pub items: Vec<Constraint>,
}

/// -----------------------------
/// Value
/// -----------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Object(HashMap<String, Value>),
    Array(Vec<Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        if let Value::String(s) = self { Some(s) } else { None }
    }
    pub fn as_int(&self) -> Option<i64> {
        if let Value::Int(i) = self { Some(*i) } else { None }
    }
    pub fn as_float(&self) -> Option<f64> {
        if let Value::Float(f) = self { Some(*f) } else { None }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }
    pub fn as_object(&self) -> Option<&HashMap<String, Value>> {
        if let Value::Object(m) = self { Some(m) } else { None }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut HashMap<String, Value>> {
        if let Value::Object(m) = self { Some(m) } else { None }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
}

#[derive(Debug, Clone)]
pub struct FieldRule {
    pub field: String,
    pub field_type: FieldType,
    pub required: bool,
    pub default: Option<Value>,
    pub enum_values: Option<Vec<Value>>,
    pub union_types: Option<Vec<FieldType>>,
    pub constraints: Option<Constraints>,
    pub rule: Option<Box<FieldRule>>,
    pub children: Option<Vec<FieldRule>>,
    pub is_array: bool,
}
