use std::collections::HashMap;
use regex::Regex;

/// -----------------------------
/// Tokenizer
/// -----------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(f64),
    Colon,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Question,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '(' => { tokens.push(Token::LParen); chars.next(); }
            ')' => { tokens.push(Token::RParen); chars.next(); }
            '[' => { tokens.push(Token::LBracket); chars.next(); }
            ']' => { tokens.push(Token::RBracket); chars.next(); }
            ',' => { tokens.push(Token::Comma); chars.next(); }
            '?' => { tokens.push(Token::Question); chars.next(); }
            ':' => { tokens.push(Token::Colon); chars.next(); }
            '0'..='9' | '.' => {
                let mut num_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' { num_str.push(c); chars.next(); } else { break; }
                }
                let num: f64 = num_str.parse().map_err(|_| format!("Invalid number '{}'", num_str))?;
                tokens.push(Token::Number(num));
            }
            c if c.is_alphanumeric() || c == '_' => {
                let mut ident = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2.is_alphanumeric() || c2 == '_' { ident.push(c2); chars.next(); } else { break; }
                }
                if ident == "regex" && matches!(chars.peek(), Some(&'(')) {
                    chars.next();
                    if chars.next() != Some('"') { return Err("regex expects string literal".into()); }
                    let mut pattern = String::new();
                    while let Some(ch2) = chars.next() {
                        if ch2 == '"' { break; }
                        if ch2 == '\\' {
                            if let Some(next_ch) = chars.next() { pattern.push(next_ch); }
                        } else { pattern.push(ch2); }
                    }
                    if chars.next() != Some(')') { return Err("regex missing closing ')'".into()); }
                    tokens.push(Token::Ident(format!("regex({})", pattern)));
                } else {
                    tokens.push(Token::Ident(ident));
                }
            }
            c if c.is_whitespace() => { chars.next(); }
            _ => { return Err(format!("Unexpected char '{}'", ch)); }
        }
    }

    Ok(tokens)
}

/// -----------------------------
/// FieldRule AST
/// -----------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType { String, Int, Float, Bool, Object, Array }

#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    Range { min: f64, max: f64, min_inclusive: bool, max_inclusive: bool },
    Regex(String),
}

#[derive(Debug, Clone)]
pub struct Constraints { pub items: Vec<Constraint> }

#[derive(Debug, Clone)]
pub struct FieldRule {
    pub field: String,
    pub field_type: FieldType,
    pub required: bool,
    pub constraints: Option<Constraints>,
    pub rule: Option<Box<FieldRule>>,
    pub is_array: bool,
}

/// -----------------------------
/// Parser
/// -----------------------------
pub struct Parser { tokens: Vec<Token>, pos: usize }

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self { Self { tokens, pos: 0 } }
    fn peek(&self) -> Option<&Token> { self.tokens.get(self.pos) }
    fn next(&mut self) -> Option<Token> { let t = self.tokens.get(self.pos).cloned(); self.pos += 1; t }
    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let t = self.next().ok_or("Unexpected EOF")?;
        if &t != expected { return Err(format!("Expected {:?}, got {:?}", expected, t)); }
        Ok(())
    }

    pub fn parse_program(&mut self) -> Result<Vec<FieldRule>, String> {
        let mut rules = Vec::new();
        while self.peek().is_some() { rules.push(self.parse_section()?); }
        Ok(rules)
    }

    fn parse_section(&mut self) -> Result<FieldRule, String> {
        self.expect(&Token::LParen)?;
        let mut sub_fields = Vec::new();
        loop {
            sub_fields.push(self.parse_field()?);
            match self.peek() {
                Some(Token::Comma) => { self.next(); }
                Some(Token::RParen) => { self.next(); break; }
                _ => return Err("Expected ',' or ')'".into()),
            }
        }

        let mut dummy_rule = FieldRule {
            field: "".to_string(),
            field_type: FieldType::Object,
            required: true,
            constraints: None,
            rule: None,
            is_array: false,
        };

        if !sub_fields.is_empty() {
            let mut root = sub_fields.into_iter().rev().fold(None, |acc, mut fr| {
                fr.rule = acc.map(Box::new);
                Some(fr)
            });
            dummy_rule.rule = root.map(Box::new);
        }

        Ok(dummy_rule)
    }

    fn parse_field(&mut self) -> Result<FieldRule, String> {
        let name = match self.next() { Some(Token::Ident(s)) => s, t => return Err(format!("Expected field name, got {:?}", t)) };
        self.expect(&Token::Colon)?;
        let field_type = match self.next() {
            Some(Token::Ident(s)) if s == "string" => FieldType::String,
            Some(Token::Ident(s)) if s == "int" => FieldType::Int,
            Some(Token::Ident(s)) if s == "float" => FieldType::Float,
            Some(Token::Ident(s)) if s == "bool" => FieldType::Bool,
            Some(Token::Ident(s)) if s == "object" => FieldType::Object,
            Some(Token::Ident(s)) if s == "array" => FieldType::Array,
            t => return Err(format!("Invalid type {:?}", t)),
        };

        let mut constraints = Vec::new();
        loop {
            match self.peek() {
                Some(Token::LParen) | Some(Token::LBracket) => constraints.push(self.parse_range_constraint()?),
                Some(Token::Ident(s)) if s.starts_with("regex(") => {
                    if let Some(Token::Ident(regex_token)) = self.next() {
                        let pattern = regex_token.trim_start_matches("regex(").trim_end_matches(')').to_string();
                        constraints.push(Constraint::Regex(pattern));
                    }
                }
                _ => break,
            }
        }

        let required = !matches!(self.peek(), Some(Token::Question));
        if !required { self.next(); }

        let is_array = field_type == FieldType::Array;

        Ok(FieldRule {
            field: name,
            field_type,
            required,
            constraints: if constraints.is_empty() { None } else { Some(Constraints { items: constraints }) },
            rule: None,
            is_array,
        })
    }

    fn parse_range_constraint(&mut self) -> Result<Constraint, String> {
        let min_inclusive = matches!(self.peek(), Some(Token::LBracket));
        let max_inclusive: bool;
        self.next();
        let min = match self.next() { Some(Token::Number(n)) => n, t => return Err(format!("Expected number for range min, got {:?}", t)) };
        self.expect(&Token::Comma)?;
        let max = match self.next() { Some(Token::Number(n)) => n, t => return Err(format!("Expected number for range max, got {:?}", t)) };
        match self.next() {
            Some(Token::RBracket) => max_inclusive = true,
            Some(Token::RParen) => max_inclusive = false,
            t => return Err(format!("Expected closing ] or ), got {:?}", t)),
        }
        Ok(Constraint::Range { min, max, min_inclusive, max_inclusive })
    }
}

pub fn parse_rules(input: &str) -> Result<Vec<FieldRule>, String> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

/// -----------------------------
/// Value + Validator
/// -----------------------------
#[derive(Debug, Clone)]
pub enum Value { String(String), Int(f64), Float(f64), Bool(bool), Object(HashMap<String, Value>), Array(Vec<Value>) }

impl Value {
    pub fn as_str(&self) -> Option<&str> { if let Value::String(s) = self { Some(s) } else { None } }
    pub fn as_int(&self) -> Option<f64> { if let Value::Int(i) = self { Some(*i) } else { None } }
    pub fn as_float(&self) -> Option<f64> { if let Value::Float(f) = self { Some(*f) } else { None } }
    pub fn as_bool(&self) -> Option<bool> { if let Value::Bool(b) = self { Some(*b) } else { None } }
    pub fn as_object(&self) -> Option<&HashMap<String, Value>> { if let Value::Object(m) = self { Some(m) } else { None } }
    pub fn as_array(&self) -> Option<&Vec<Value>> { if let Value::Array(a) = self { Some(a) } else { None } }
}

pub fn validate_field(value: &Value, rule: &FieldRule) -> Result<(), String> {
    if !rule.required {
        if let Value::Object(obj) = value {
            if !obj.contains_key(&rule.field) { return Ok(()); }
        }
    }

    match rule.field_type {
        FieldType::String => {
            let s = value.as_str().ok_or(format!("{} not string", rule.field))?;
            if let Some(c) = &rule.constraints {
                for con in &c.items {
                    match con {
                        Constraint::Range { min, max, min_inclusive, max_inclusive } => {
                            let len = s.chars().count() as f64;
                            let min_ok = if *min_inclusive { len >= *min } else { len > *min };
                            let max_ok = if *max_inclusive { len <= *max } else { len < *max };
                            if !min_ok || !max_ok { return Err(format!("{} length {} out of range", rule.field, len)); }
                        }
                        Constraint::Regex(pattern) => {
                            let re = Regex::new(pattern).map_err(|e| format!("Invalid regex: {}", e))?;
                            if !re.is_match(s) { return Err(format!("{} regex mismatch: {}", rule.field, pattern)); }
                        }
                    }
                }
            }
        }
        FieldType::Int | FieldType::Float => {
            let n = match rule.field_type {
                FieldType::Int => value.as_int().ok_or(format!("{} not int", rule.field))?,
                FieldType::Float => value.as_float().ok_or(format!("{} not float", rule.field))?,
                _ => unreachable!(),
            };
            if let Some(c) = &rule.constraints {
                for con in &c.items {
                    if let Constraint::Range { min, max, min_inclusive, max_inclusive } = con {
                        let min_ok = if *min_inclusive { n >= *min } else { n > *min };
                        let max_ok = if *max_inclusive { n <= *max } else { n < *max };
                        if !min_ok || !max_ok { return Err(format!("{} value {} out of range", rule.field, n)); }
                    }
                }
            }
        }
        FieldType::Bool => { value.as_bool().ok_or(format!("{} not bool", rule.field))?; }
        FieldType::Object => {
            let obj = value.as_object().ok_or(format!("{} not object", rule.field))?;
            if let Some(sub_rule) = &rule.rule {
                let mut current = Some(sub_rule.as_ref());
                while let Some(fr) = current {
                    if let Some(v) = obj.get(&fr.field) { validate_field(v, fr)?; } 
                    else if fr.required { return Err(format!("Missing required field {}", fr.field)); }
                    current = fr.rule.as_deref();
                }
            }
        }
        FieldType::Array => {
            let arr = value.as_array().ok_or(format!("{} not array", rule.field))?;
            if let Some(sub_rule) = &rule.rule {
                for v in arr.iter() { validate_field(v, sub_rule)?; }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_dsl() {
        let dsl = r#"(age:int[0,150],score:float(0,100),username:string[2,20],tags:array)"#;
        let rules = parse_rules(dsl).unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn test_validate_range() {
        let dsl = r#"(age:int[0,150],score:float(0,100),username:string[2,20])"#;
        let rules = parse_rules(dsl).unwrap();
        let mut map = HashMap::new();
        map.insert("age".to_string(), Value::Int(30.0));
        map.insert("score".to_string(), Value::Float(99.5));
        map.insert("username".to_string(), Value::String("user_01".to_string()));
        let value = Value::Object(map);

        for rule in &rules {
            if let Some(sub_rule) = &rule.rule {
                let mut current = Some(sub_rule.as_ref());
                while let Some(fr) = current {
                    let v = value.as_object().unwrap().get(&fr.field).unwrap();
                    validate_field(v, fr).unwrap();
                    current = fr.rule.as_deref();
                }
            }
        }
    }

    #[test]
    fn test_validate_optional_field() {
        let dsl = r#"(username:string[2,20],nickname:string[0,20]?)"#;
        let rules = parse_rules(dsl).unwrap();

        let mut map = HashMap::new();
        map.insert("username".to_string(), Value::String("user01".to_string()));
        let value = Value::Object(map);

        for rule in &rules {
            if let Some(sub_rule) = &rule.rule {
                let mut current = Some(sub_rule.as_ref());
                while let Some(fr) = current {
                    if let Some(v) = value.as_object().unwrap().get(&fr.field) {
                        validate_field(v, fr).unwrap();
                    }
                    current = fr.rule.as_deref();
                }
            }
        }

        let mut map2 = HashMap::new();
        map2.insert("username".to_string(), Value::String("user01".to_string()));
        map2.insert("nickname".to_string(), Value::String("nick".to_string()));
        let value2 = Value::Object(map2);

        for rule in &rules {
            if let Some(sub_rule) = &rule.rule {
                let mut current = Some(sub_rule.as_ref());
                while let Some(fr) = current {
                    let v = value2.as_object().unwrap().get(&fr.field).unwrap();
                    validate_field(v, fr).unwrap();
                    current = fr.rule.as_deref();
                }
            }
        }
    }

    #[test]
    fn test_validate_regex_success() {
        let dsl = r#"(username:string[3,20]regex("^[a-zA-Z0-9_]+$"))"#;
        let rules = parse_rules(dsl).unwrap();

        let mut map = HashMap::new();
        map.insert("username".to_string(), Value::String("user_123".to_string()));
        let value = Value::Object(map);

        for rule in &rules {
            if let Some(sub_rule) = &rule.rule {
                let mut current = Some(sub_rule.as_ref());
                while let Some(fr) = current {
                    let v = value.as_object().unwrap().get(&fr.field).unwrap();
                    validate_field(v, fr).unwrap();
                    current = fr.rule.as_deref();
                }
            }
        }
    }

    #[test]
    fn test_validate_regex_fail() {
        let fr = FieldRule {
            field: "username".to_string(),
            field_type: FieldType::String,
            required: true,
            constraints: Some(Constraints {
                items: vec![
                    Constraint::Range { min: 3.0, max: 20.0, min_inclusive: true, max_inclusive: true },
                    Constraint::Regex("^[a-zA-Z0-9_]+$".to_string())
                ],
            }),
            rule: None,
            is_array: false,
        };

        let value = Value::String("invalid-username!".to_string());
        assert!(validate_field(&value, &fr).is_err());
    }

    #[test]
    fn test_validate_bool_success() {
        let fr = FieldRule { field: "active".to_string(), field_type: FieldType::Bool, required: true, constraints: None, rule: None, is_array: false };
        let value = Value::Bool(true);
        validate_field(&value, &fr).unwrap();
    }

    #[test]
    fn test_validate_bool_fail() {
        let fr = FieldRule { field: "active".to_string(), field_type: FieldType::Bool, required: true, constraints: None, rule: None, is_array: false };
        let value = Value::String("true".to_string());
        assert!(validate_field(&value, &fr).is_err());
    }

    #[test]
    fn test_validate_object_success() {
        let fr = FieldRule { field: "profile".to_string(), field_type: FieldType::Object, required: true, constraints: None, rule: None, is_array: false };
        let mut map = HashMap::new();
        map.insert("name".to_string(), Value::String("Alice".to_string()));
        let value = Value::Object(map);
        validate_field(&value, &fr).unwrap();
    }

    #[test]
    fn test_validate_array_success() {
        let fr = FieldRule { field: "tags".to_string(), field_type: FieldType::Array, required: true, constraints: None, rule: None, is_array: true };
        let value = Value::Array(vec![Value::String("rust".to_string()), Value::String("dsl".to_string())]);
        validate_field(&value, &fr).unwrap();
    }
}
