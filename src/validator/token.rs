/// -----------------------------
/// Tokenizer
/// -----------------------------
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Number(String), // 数字统一存为字符串，包括科学计数法
    Colon,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Question,
    Lt,
    Gt,
    Enum,
    Equal,
    Pipe,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            '[' => {
                tokens.push(Token::LBracket);
                chars.next();
            }
            ']' => {
                tokens.push(Token::RBracket);
                chars.next();
            }
            '<' => {
                tokens.push(Token::Lt);
                chars.next();
            }
            '>' => {
                tokens.push(Token::Gt);
                chars.next();
            }
            ',' => {
                tokens.push(Token::Comma);
                chars.next();
            }
            '?' => {
                tokens.push(Token::Question);
                chars.next();
            }
            ':' => {
                tokens.push(Token::Colon);
                chars.next();
            }
            '=' => {
                tokens.push(Token::Equal);
                chars.next();
            }
            '|' => {
                tokens.push(Token::Pipe);
                chars.next();
            }

            // 新逻辑：支持 + / - 开头
            '0'..='9' | '.' | '+' | '-' => {
                let mut num_str = String::new();
                // 如果开头是 + 或 -，先记录并移动
                if let Some(&c) = chars.peek() {
                    if c == '+' || c == '-' {
                        num_str.push(c);
                        chars.next();
                    }
                }

                while let Some(&c) = chars.peek() {
                    // 数字主体部分，包括科学计数法 e/E 和可能的 +/-
                    if
                        c.is_ascii_digit() ||
                        c == '.' ||
                        c == 'e' ||
                        c == 'E' ||
                        c == '+' ||
                        c == '-'
                    {
                        num_str.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }

                // 尝试解析为 f64 验证格式是否正确
                if num_str.parse::<f64>().is_err() {
                    return Err(format!("Invalid number '{}'", num_str));
                }

                tokens.push(Token::Number(num_str));
            }
            '"' => {
                chars.next(); // skip opening quote
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '"' {
                        chars.next(); // skip closing quote
                        break;
                    }
                    // 支持转义字符
                    if c == '\\' {
                        chars.next();
                        if let Some(&esc) = chars.peek() {
                            let esc_ch = match esc {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                '"' => '"',
                                '\\' => '\\',
                                other => other,
                            };
                            s.push(esc_ch);
                            chars.next();
                        }
                    } else {
                        s.push(c);
                        chars.next();
                    }
                }
                tokens.push(Token::Ident(s)); // 字符串作为 Ident 保存
            }
            c if c.is_alphanumeric() || c == '_' => {
                let mut ident = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2.is_alphanumeric() || c2 == '_' {
                        ident.push(c2);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Ident(ident));
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            _ => {
                return Err(format!("Unexpected char '{}'", ch));
            }
        }
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_full_dsl_with_scientific_range() {
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

        let tokens = tokenize(dsl).expect("Failed to tokenize DSL");

        let expected_tokens = vec![
            Token::LParen,
            Token::Ident("username".into()),
            Token::Colon,
            Token::Ident("string".into()),
            Token::LBracket,
            Token::Number("3".into()),
            Token::Comma,
            Token::Number("20".into()),
            Token::RBracket,
            Token::Ident("regex".into()),
            Token::LParen,
            Token::Ident("^[a-zA-Z0-9_]+$".into()),
            Token::RParen,
            Token::Comma,

            Token::Ident("age".into()),
            Token::Colon,
            Token::Ident("int".into()),
            Token::LBracket,
            Token::Number("0".into()),
            Token::Comma,
            Token::Number("150".into()),
            Token::RBracket,
            Token::Equal,
            Token::Number("30".into()),
            Token::Comma,

            Token::Ident("age".into()),
            Token::Colon,
            Token::Ident("int".into()),
            Token::Equal,
            Token::Number("30".into()),
            Token::Comma,

            Token::Ident("score".into()),
            Token::Colon,
            Token::Ident("float".into()),
            Token::LParen,
            Token::Number("0".into()),
            Token::Comma,
            Token::Number("100".into()),
            Token::RParen,
            Token::Comma,

            Token::Ident("active".into()),
            Token::Colon,
            Token::Ident("bool".into()),
            Token::Equal,
            Token::Ident("true".into()),
            Token::Comma,

            Token::Ident("nickname".into()),
            Token::Question,
            Token::Colon,
            Token::Ident("string".into()),
            Token::LBracket,
            Token::Number("0".into()),
            Token::Comma,
            Token::Number("20".into()),
            Token::RBracket,
            Token::Comma,

            Token::Ident("role".into()),
            Token::Colon,
            Token::Ident("string".into()),
            Token::Ident("enum".into()),
            Token::LParen,
            Token::Ident("admin".into()),
            Token::Comma,
            Token::Ident("user".into()),
            Token::Comma,
            Token::Ident("guest".into()),
            Token::RParen,
            Token::Equal,
            Token::Ident("user".into()),
            Token::Comma,

            Token::Ident("id".into()),
            Token::Colon,
            Token::Ident("int".into()),
            Token::Pipe,
            Token::Ident("float".into()),
            Token::Comma,

            Token::Ident("profile".into()),
            Token::Colon,
            Token::Ident("object".into()),
            Token::LParen,
            Token::Ident("first_name".into()),
            Token::Colon,
            Token::Ident("string".into()),
            Token::LBracket,
            Token::Number("1".into()),
            Token::Comma,
            Token::Number("50".into()),
            Token::RBracket,
            Token::Comma,
            Token::Ident("last_name".into()),
            Token::Colon,
            Token::Ident("string".into()),
            Token::LBracket,
            Token::Number("1".into()),
            Token::Comma,
            Token::Number("50".into()),
            Token::RBracket,
            Token::Comma,
            Token::Ident("contact".into()),
            Token::Colon,
            Token::Ident("object".into()),
            Token::LParen,
            Token::Ident("email".into()),
            Token::Colon,
            Token::Ident("string".into()),
            Token::Ident("regex".into()),
            Token::LParen,
            Token::Ident("^[^@\\s]+@[^@\\s]+\\.[^@\\s]+$".into()),
            Token::RParen,
            Token::Comma,
            Token::Ident("phone".into()),
            Token::Question,
            Token::Colon,
            Token::Ident("string".into()),
            Token::LBracket,
            Token::Number("0".into()),
            Token::Comma,
            Token::Number("20".into()),
            Token::RBracket,
            Token::RParen,
            Token::RParen,
            Token::Comma,

            Token::Ident("tags".into()),
            Token::Colon,
            Token::Ident("array".into()),
            Token::Lt,
            Token::Ident("string".into()),
            Token::LBracket,
            Token::Number("1".into()),
            Token::Comma,
            Token::Number("10".into()),
            Token::RBracket,
            Token::Gt,
            Token::Comma,

            Token::Ident("scores".into()),
            Token::Colon,
            Token::Ident("array".into()),
            Token::Lt,
            Token::Ident("int".into()),
            Token::LBracket,
            Token::Number("0".into()),
            Token::Comma,
            Token::Number("100".into()),
            Token::RBracket,
            Token::Gt,
            Token::Comma,

            // 新增科学计数法
            Token::Ident("distance".into()),
            Token::Colon,
            Token::Ident("float".into()),
            Token::LBracket,
            Token::Number("1.47e11".into()),
            Token::Comma,
            Token::Number("1.52e11".into()),
            Token::RBracket,
            Token::Equal,
            Token::Number("1.496e11".into()),
            Token::Comma,

            Token::Ident("positive_scientific".into()),
            Token::Colon,
            Token::Ident("float".into()),
            Token::LBracket,
            Token::Number("+1.0e3".into()),
            Token::Comma,
            Token::Number("+2.0E3".into()),
            Token::RBracket,
            Token::Equal,
            Token::Number("+1.5e3".into()),
            Token::Comma,

            Token::Ident("negative_scientific".into()),
            Token::Colon,
            Token::Ident("float".into()),
            Token::LBracket,
            Token::Number("-1.0e3".into()),
            Token::Comma,
            Token::Number("-2.0E3".into()),
            Token::RBracket,
            Token::Equal,
            Token::Number("-1.5e3".into()),
            Token::Comma,

            Token::Ident("mixed_sign_scientific".into()),
            Token::Colon,
            Token::Ident("float".into()),
            Token::LBracket,
            Token::Number("-1.0e3".into()),
            Token::Comma,
            Token::Number("+2.0e3".into()),
            Token::RBracket,
            Token::Equal,
            Token::Number("3.0e0".into()),
            Token::Comma,

            Token::Ident("escaped_field".into()),
            Token::Colon,
            Token::Ident("string".into()),
            Token::Ident("regex".into()),
            Token::LParen,
            Token::Ident("line1\nline2\rtab\tquote\"backslash\\".into()),
            Token::RParen,
            Token::Comma,

            // field_with_underscore
            Token::Ident("_start_with_underscore".into()),
            Token::Colon,
            Token::Ident("string".into()),
            Token::LBracket,
            Token::Number("1".into()),
            Token::Comma,
            Token::Number("10".into()),
            Token::RBracket,
            Token::Equal,
            Token::Number("5".into()),

            Token::RParen
        ];

        assert_eq!(tokens, expected_tokens, "Tokens did not match expected sequence");
    }
}
