use crate::validator::{
    ast::{ Constraint, Constraints, FieldRule, FieldType, Value },
    token::{ Token, tokenize },
};

/// -----------------------------
/// Parser
/// -----------------------------
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }
    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let t = self.next().ok_or("Unexpected EOF")?;
        if &t != expected {
            return Err(format!("Expected {:?}, got {:?}", expected, t));
        }
        Ok(())
    }

    // parse_program 修正版
    pub fn parse_program(&mut self) -> Result<Vec<FieldRule>, String> {
        self.expect(&Token::LParen)?;
        let mut rules = Vec::new();
        loop {
            if matches!(self.peek(), Some(Token::RParen)) {
                self.next();
                break;
            }
            let field = self.parse_field(false)?;
            rules.push(field);

            match self.peek() {
                Some(Token::Comma) => {
                    self.next();
                }
                Some(Token::RParen) => {}
                _ => {
                    return Err("Expected ',' or ')'".into());
                }
            }
        }
        Ok(rules)
    }

    fn parse_field(&mut self, nameless: bool) -> Result<FieldRule, String> {
        // -----------------------------
        // 1️⃣ 字段名 + optional
        // -----------------------------
        let (name, optional) = if nameless {
            (String::new(), false)
        } else {
            let name = match self.next() {
                Some(Token::Ident(s)) => s,
                t => {
                    return Err(format!("Expected field name, got {:?}", t));
                }
            };

            let optional = matches!(self.peek(), Some(Token::Question));
            if optional {
                self.next();
            }

            (name, optional)
        };

        if !nameless {
            self.expect(&Token::Colon)?;
        }

        // -----------------------------
        // 2️⃣ 解析 union 类型
        // -----------------------------
        let mut union_types = Vec::new();
        loop {
            let ty = match self.next() {
                Some(Token::Ident(s)) =>
                    match s.as_str() {
                        "string" => FieldType::String,
                        "int" => FieldType::Int,
                        "float" => FieldType::Float,
                        "bool" => FieldType::Bool,
                        "object" => FieldType::Object,
                        "array" => FieldType::Array,
                        t => {
                            return Err(format!("Unknown type {}", t));
                        }
                    }
                t => {
                    return Err(format!("Expected type, got {:?}", t));
                }
            };

            union_types.push(ty);

            if matches!(self.peek(), Some(Token::Pipe)) {
                self.next();
            } else {
                break;
            }
        }

        let field_type = union_types[0].clone();

        let mut sub_rule = None;
        let mut children = None;
        let mut constraints = Vec::new();
        let mut enum_values = None;
        let mut default = None;
        let is_array = field_type == FieldType::Array;

        //
        // 3️⃣ array<sub_rule>
        //
        // 修正版 array 解析，确保 sub_rule 不被丢弃
        if is_array && matches!(self.peek(), Some(Token::Lt)) {
            self.next(); // consume '<'
            // 使用 nameless=true 避免重复解析字段名，但保留 FieldType、constraints 等
            let sub = self.parse_field(true)?;
            // 父级 array 的 rule 指向这个子规则
            sub_rule = Some(
                Box::new(FieldRule {
                    field: String::new(), // nameless
                    field_type: sub.field_type,
                    required: sub.required,
                    default: sub.default,
                    enum_values: sub.enum_values,
                    union_types: sub.union_types,
                    constraints: sub.constraints,
                    rule: sub.rule,
                    children: sub.children,
                    is_array: sub.is_array,
                })
            );
            self.expect(&Token::Gt)?;
        }

        //
        // 4️⃣ object(...)
        //
        if field_type == FieldType::Object && matches!(self.peek(), Some(Token::LParen)) {
            self.next(); // consume '('
            let mut inner = Vec::new();

            loop {
                if matches!(self.peek(), Some(Token::RParen)) {
                    self.next(); // consume ')'
                    break;
                }

                inner.push(self.parse_field(false)?);

                match self.peek() {
                    Some(Token::Comma) => {
                        self.next();
                    }
                    Some(Token::RParen) => {}
                    _ => {
                        return Err("Expected ',' or ')' in object".into());
                    }
                }
            }

            children = Some(inner);
        }

        //
        // 5️⃣ 约束 / regex / enum / default
        //
        loop {
            match self.peek() {
                // range
                Some(Token::LBracket) => {
                    constraints.push(self.parse_range(&field_type)?);
                }

                Some(Token::LParen) => {
                    if field_type == FieldType::Object {
                        return Err("Unexpected '(' after object definition".into());
                    }
                    constraints.push(self.parse_range(&field_type)?);
                }

                // regex
                Some(Token::Ident(s)) if s == "regex" => {
                    self.next();
                    self.expect(&Token::LParen)?;
                    let pattern = match self.next() {
                        Some(Token::Ident(p)) => p,
                        t => {
                            return Err(format!("Expected pattern, got {:?}", t));
                        }
                    };
                    self.expect(&Token::RParen)?;
                    constraints.push(Constraint::Regex(pattern));
                }

                // enum
                Some(Token::Ident(s)) if s == "enum" => {
                    self.next();
                    self.expect(&Token::LParen)?;
                    let mut vals = Vec::new();

                    loop {
                        match self.next() {
                            Some(Token::Ident(v)) => vals.push(Value::String(v)),
                            t => {
                                return Err(format!("Expected enum value, got {:?}", t));
                            }
                        }

                        match self.peek() {
                            Some(Token::Comma) => {
                                self.next();
                            }
                            Some(Token::RParen) => {
                                self.next();
                                break;
                            }
                            _ => {
                                return Err("Expected ',' or ')' in enum".into());
                            }
                        }
                    }

                    enum_values = Some(vals);
                }

                // default
                Some(Token::Equal) => {
                    self.next();
                    let token = self.next().ok_or("Expected default value")?;

                    let val = match token {
                        Token::Number(s) => {
                            self.parse_token_number_as_type(&Token::Number(s), &field_type)?
                        }
                        Token::Ident(s) => {
                            if field_type == FieldType::Bool {
                                match s.as_str() {
                                    "true" => Value::Bool(true),
                                    "false" => Value::Bool(false),
                                    _ => {
                                        return Err(format!("Invalid bool '{}'", s));
                                    }
                                }
                            } else {
                                Value::String(s)
                            }
                        }
                        t => {
                            return Err(format!("Unexpected default value {:?}", t));
                        }
                    };

                    default = Some(val);
                }

                _ => {
                    break;
                }
            }
        }

        Ok(FieldRule {
            field: name,
            field_type,
            required: if nameless {
                true
            } else {
                !optional
            },
            default,
            enum_values,
            union_types: if union_types.len() > 1 {
                Some(union_types)
            } else {
                None
            },
            constraints: if constraints.is_empty() {
                None
            } else {
                Some(Constraints { items: constraints })
            },
            rule: sub_rule,
            children,
            is_array,
        })
    }

    /// 根据 FieldType 解析 Token::Number 为 Value
    fn parse_token_number_as_type(
        &self,
        token: &Token,
        field_type: &FieldType
    ) -> Result<Value, String> {
        match token {
            Token::Number(s) =>
                match field_type {
                    FieldType::String => Ok(Value::String(s.to_string())), // <- 允许数字作为 String
                    FieldType::Int => {
                        s.parse::<i64>()
                            .map(Value::Int)
                            .map_err(|_| format!("Invalid integer '{}'", s))
                    }
                    FieldType::Float => {
                        s.parse::<f64>()
                            .map(Value::Float)
                            .map_err(|_| format!("Invalid float '{}'", s))
                    }
                    _ => Err(format!("Field type {:?} cannot parse number", field_type)),
                }
            _ => Err(format!("Expected number token, got {:?}", token)),
        }
    }

    /// Range 解析，支持 int/float
    fn parse_range(&mut self, field_type: &FieldType) -> Result<Constraint, String> {
        let min_inclusive = matches!(self.peek(), Some(Token::LBracket));
        self.next();

        let min_token = self.next().ok_or("Expected min number")?;
        let min = self.parse_token_number_as_type(&min_token, field_type)?;

        self.expect(&Token::Comma)?;

        let max_token = self.next().ok_or("Expected max number")?;
        let max = self.parse_token_number_as_type(&max_token, field_type)?;

        let max_inclusive = match self.next() {
            Some(Token::RBracket) => true,
            Some(Token::RParen) => false,
            t => {
                return Err(format!("Expected closing bracket or paren, got {:?}", t));
            }
        };

        Ok(Constraint::Range { min, max, min_inclusive, max_inclusive })
    }
}

pub fn parse_rules(input: &str) -> Result<Vec<FieldRule>, String> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn test_full_dsl_parse() {
        let dsl =
            r#"
        (
            username:string[3,20] regex("^[a-zA-Z0-9_]+$"),  
            age:int[0,150]=30,      
            age:int=30,    
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
            scores:array<int[0,100]>,
            distance:float[1.47e11,1.52e11]=1.496e11,
            positive_scientific:float[+1.0e3,+2.0E3]=+1.5e3, 
            negative_scientific:float[-1.0e3,-2.0E3]=-1.5e3, 
            mixed_sign_scientific:float[-1.0e3,+2.0e3]=3.0e0,
            escaped_field:string regex("line1\nline2\rtab\tquote\"backslash\\"),
            _start_with_underscore:string[1,10]=5
        )
        "#;

        let rules = parse_rules(dsl).expect("Failed to parse DSL");

        // 检查总字段数量
        assert_eq!(rules.len(), 17);

        // 检查部分关键字段
        assert_eq!(rules[0].field, "username");
        assert_eq!(rules[0].field_type, FieldType::String);
        assert!(rules[0].constraints.is_some());

        assert_eq!(rules[16].field, "_start_with_underscore");
        assert_eq!(rules[16].field_type, FieldType::String);
        assert_eq!(rules[16].default, Some(Value::String("5".into())));

        let escaped_field_constraint = match &rules[15].constraints {
            Some(Constraints { items }) =>
                match &items[0] {
                    Constraint::Regex(p) => p,
                    _ => panic!("Expected regex constraint"),
                }
            _ => panic!("Expected constraints"),
        };
        println!("{:?}", escaped_field_constraint);
        // 确认转义字符仍存在
        assert!(escaped_field_constraint.contains(r"line1"));
        assert!(escaped_field_constraint.contains("\n"));
        // assert!(escaped_field_constraint.contains(r"quote"));
    }
}
